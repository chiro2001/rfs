/// Filesystem logics
use std::cmp::max;
use std::mem::size_of;
use std::time::Duration;
pub use disk_driver;
use anyhow::{anyhow, Result};
use disk_driver::{DiskDriver, DiskInfo, SeekType};
use log::*;
use num::range_step;

#[macro_use]
pub mod utils;
pub mod desc;
pub mod types;
pub mod mem;
pub mod fs;

use utils::*;
use mem::*;
use desc::*;
use crate::prv;

#[cxx::bridge]
mod ffi {
    extern "Rust" {
        fn add(left: usize, right: usize) -> usize;
    }
}

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

/// Data TTL, 1 second default
const TTL: Duration = Duration::from_secs(1);

pub struct RFS {
    pub driver: Box<dyn DiskDriver>,
    pub driver_info: DiskInfo,
    pub super_block: Ext2SuperBlockMem,
    pub group_desc_table: Vec<Ext2GroupDesc>,
    /// ext2 may has boot reserved 1 block prefix
    pub filesystem_first_block: usize,
    /// bitmap in memory
    pub bitmap_inode: Vec<u8>,
    pub bitmap_data: Vec<u8>,
    /// Root directory
    pub root_dir: Ext2INode,
}

impl RFS {
    /// Create RFS object from selected DiskDriver
    #[allow(dead_code)]
    pub fn new(driver: Box<dyn DiskDriver>) -> Self {
        Self {
            driver,
            driver_info: Default::default(),
            super_block: Default::default(),
            group_desc_table: vec![],
            filesystem_first_block: 0,
            bitmap_inode: vec![],
            bitmap_data: vec![],
            root_dir: Default::default(),
        }
    }

    /// Get disk unit, available after init
    fn disk_block_size(self: &Self) -> usize { self.driver_info.consts.iounit_size as usize }

    /// Get disk sizs, available after init
    fn disk_size(self: &Self) -> usize { self.driver_info.consts.layout_size as usize }

    /// Get filesystem block size, available after init
    fn block_size(self: &Self) -> usize { (1 << self.super_block.s_log_block_size) * 0x400 as usize }

    /// Read one disk block
    fn read_disk_block(self: &mut Self, buf: &mut [u8]) -> Result<()> {
        assert_eq!(buf.len(), self.disk_block_size());
        let sz = self.disk_block_size();
        self.driver.ddriver_read(buf, sz)?;
        Ok(())
    }

    /// Write one disk block
    fn write_disk_block(self: &mut Self, buf: &[u8]) -> Result<()> {
        assert_eq!(buf.len(), self.disk_block_size());
        let sz = self.disk_block_size();
        self.driver.ddriver_write(buf, sz)?;
        Ok(())
    }

    /// Read multi disk units from disk
    fn read_disk_blocks(self: &mut Self, buf: &mut [u8], count: usize) -> Result<()> {
        let sz = self.disk_block_size();
        for i in 0..count { self.read_disk_block(&mut buf[(i * sz)..((i + 1) * sz)])? }
        Ok(())
    }

    /// Write multi disk units from disk
    fn write_disk_blocks(self: &mut Self, buf: &[u8], count: usize) -> Result<()> {
        let sz = self.disk_block_size();
        for i in 0..count { self.write_disk_block(&buf[(i * sz)..((i + 1) * sz)])? }
        Ok(())
    }

    /// Seek disk cursor by bytes
    fn seek_disk_block(self: &mut Self, index: usize) -> Result<()> {
        let sz = self.disk_block_size();
        // info!("DISK seek to {:x}", index * sz);
        let _n = self.driver.ddriver_seek((index * sz) as i64, SeekType::Set)?;
        Ok(())
    }

    /// How many disk unit for one filesystem block.
    /// fs block size should larger than ont disk unit
    fn block_disk_ratio(self: &Self) -> usize { self.block_size() / self.disk_block_size() }

    /// Seek disk by unit of fs block size
    pub fn seek_block(self: &mut Self, index: usize) -> Result<()> {
        self.seek_disk_block(index * self.block_disk_ratio())
    }

    /// Read disk by one block
    pub fn read_block(self: &mut Self, buf: &mut [u8]) -> Result<()> {
        self.read_disk_blocks(buf, self.block_disk_ratio())
    }

    /// Write disk by one block
    pub fn write_block(self: &mut Self, buf: &[u8]) -> Result<()> {
        self.write_disk_blocks(buf, self.block_disk_ratio())
    }

    /// Read disk by multi-blocks
    #[allow(dead_code)]
    pub fn read_blocks(self: &mut Self, buf: &mut [u8], count: usize) -> Result<()> {
        self.read_disk_blocks(buf, self.block_disk_ratio() * count)
    }

    /// Write disk by multi-blocks
    #[allow(dead_code)]
    pub fn write_blocks(self: &mut Self, buf: &[u8], count: usize) -> Result<()> {
        self.write_disk_blocks(buf, self.block_disk_ratio() * count)
    }

    /// Create a Vec<u8> in block size
    pub fn create_block_vec(self: &mut Self) -> Vec<u8> {
        [0 as u8].repeat(self.block_size())
    }

    /// Create a Vec<u8> in multi-blocks size
    #[allow(dead_code)]
    pub fn create_blocks_vec(self: &Self, count: usize) -> Vec<u8> {
        [0 as u8].repeat(self.block_size() * count)
    }

    /// Get `Ext2GroupDesc`, available after init
    fn get_group_desc(self: &Self) -> &Ext2GroupDesc {
        self.group_desc_table.get(0).unwrap()
    }

    /// Print basic fs info
    pub fn print_stats(self: &Self) {
        info!("fs stats: {}", self.super_block.to_string());
    }

    /// Calculate block number and offset in a block for inode
    fn fetch_inode_block_offset(self: &Self, ino: usize) -> Result<(usize, usize)> {
        // should ino minus 1?
        let inodes_per_block = self.block_size() / EXT2_INODE_SIZE;
        // assert only one group
        // let block_group = (ino - 1) / inodes_per_block;
        let ino = if ino <= 1 { ino } else { ino - 1 };
        let offset = (ino % inodes_per_block) * EXT2_INODE_SIZE;
        let block_number = ino / inodes_per_block + self.get_group_desc().bg_inode_table as usize;
        // prv!(ino, block_number, offset / EXT2_INODE_SIZE);
        Ok((block_number, offset))
    }

    /// Read inode struct according to ino number
    pub fn get_inode(self: &mut Self, ino: usize) -> Result<Ext2INode> {
        let (block_number, offset) = self.fetch_inode_block_offset(ino)?;
        let mut buf = self.create_block_vec();
        self.seek_block(block_number)?;
        self.read_block(&mut buf)?;
        Ok(unsafe { deserialize_row(&buf[offset..]) })
    }

    /// Write inode struct according to ino number
    pub fn set_inode(self: &mut Self, ino: usize, inode: &Ext2INode) -> Result<()> {
        let (block_number, offset) = self.fetch_inode_block_offset(ino)?;
        let mut buf = self.create_block_vec();
        self.seek_block(block_number)?;
        self.read_block(&mut buf)?;
        self.seek_block(block_number)?;
        buf[offset..offset + size_of::<Ext2INode>()]
            .copy_from_slice(unsafe { serialize_row(inode) });
        self.write_block(&buf)?;
        Ok(())
    }

    /// Read one data block and return one Vec<u8>
    pub fn get_data_block(self: &mut Self, block: usize) -> Result<Vec<u8>> {
        self.seek_block(block)?;
        let mut buf = self.create_block_vec();
        self.read_block(&mut buf)?;
        Ok(buf)
    }

    /// Read one data block to mutable slice inplace
    pub fn read_data_block(self: &mut Self, block: usize, buf: &mut [u8]) -> Result<()> {
        self.seek_block(block)?;
        self.read_block(buf)?;
        Ok(())
    }

    /// Write one data block from slice inplace
    pub fn write_data_block(self: &mut Self, block: usize, buf: &[u8]) -> Result<()> {
        self.seek_block(block)?;
        self.write_block(buf)?;
        Ok(())
    }

    /// Read all directory entries in one block
    pub fn get_block_dir_entries(self: &mut Self, block: usize) -> Result<Vec<Ext2DirEntry>> {
        if block == 0 { return Ok(vec![]); }
        let data_block = self.get_data_block(block)?;
        let mut p = 0;
        let mut dirs = vec![];
        while p <= data_block.len() {
            let dir: Ext2DirEntry = unsafe { deserialize_row(&data_block[p..]) };
            if dir.inode == 0 || dir.inode >= self.super_block.s_inodes_count || dir.rec_len == 0 {
                break;
            }
            debug!("[p {:x}] name_len = {}, rec_len = {}", p, dir.name_len, dir.rec_len);
            p += dir.rec_len as usize;
            debug!("next p: {:x}; dir: {}", p, dir.to_string());
            dirs.push(dir);
        }
        if !dirs.is_empty() { debug!("last dir entry: {} {:?}", dirs.last().unwrap().to_string(), dirs.last().unwrap()); }
        Ok(dirs)
    }

    /// Read all directory entries by ino
    pub fn get_dir_entries(self: &mut Self, ino: usize) -> Result<Vec<Ext2DirEntry>> {
        let inode = self.get_inode(ino)?;
        prv!(inode);
        // TODO: walk all blocks, including indirect blocks
        // let offset = offset as usize;
        // let size = size as usize;
        // let sz = self.block_size();
        // let ino = RFS::shift_ino(ino);
        //
        // let mut blocks: Vec<usize> = vec![];
        //
        // rep!(reply, self.walk_blocks_inode(ino, offset / self.block_size(), &mut |block, index| {
        //     debug!("walk to block {} index {}", block, index);
        //     blocks.push(block);
        //     Ok(index * sz < size)
        // }));

        Ok(inode.i_block.iter().take(12)
            .map(|b| match self.get_block_dir_entries(*b as usize) {
                Ok(e) => e,
                Err(_) => vec![]
            }).into_iter()
            .filter(|x| !x.is_empty()).flatten().collect())
    }

    /// Block index layer threshold
    pub fn threshold(self: &Self, l: usize) -> usize {
        let layer = self.block_size() / 4;
        match l {
            0 => 12,
            1 => 12 + layer,
            2 => 12 + layer + layer * layer,
            3 => 11 + layer + layer * 2 + layer * layer,
            _ => panic!("Walk layer out of range")
        }
    }

    pub fn threshold_diff(self: &Self, l: usize) -> usize {
        let layer = self.block_size() / 4;
        match l {
            0 => 12,
            1 => layer,
            2 => layer * layer,
            3 => layer * layer * layer,
            _ => panic!("Walk layer out of range")
        }
    }

    /// Walk on *ONE* Layer
    pub fn walk_blocks<const L: usize, F>(self: &mut Self, start_block: usize, block_index: usize, s: usize, mut f: &mut F) -> Result<bool>
        where F: FnMut(usize, usize) -> Result<bool> {
        debug!("walk_blocks<{}>(start_block={}, block_index={})", L, start_block, block_index);
        if start_block == 0 {
            debug!("start_block is zero!");
            return Ok(false);
        }
        // m = log2(block_size / 4) = log2(layer), x / a == x >> m
        let m = self.super_block.s_log_block_size as usize + 10 - 2;
        let layer_size = self.block_size() / 4;
        // let layer_size_mask = (layer_size * 4) - 1;
        let mut data_block = self.create_block_vec();
        let mut buf_u32 = [0 as u8; 4];
        self.read_data_block(start_block, &mut data_block)?;
        // for i in block_index..(self.threshold_diff(L) + target_offset) {
        // for i in block_index..(block_index + self.threshold_diff(L)) {
        assert_eq!(self.threshold_diff(L) / (1 << (m * (s - 1))), layer_size);
        // let entry_index_start = (block_index - 12) >> ((s - 1) * m);
        let entry_index_start = self.threshold(L - 1);
        for i in range_step(entry_index_start, self.threshold(L + 1), 1 << (m * (s - 1))) {
            // for i in entry_index_start..((entry_index_start + layer_size) % layer_size) {
            // let o = ((i - self.threshold(L - 1)) >> (2 * (L - 1))) & layer_size_mask;
            // let x = i - 12;
            // let o = ((x << 2) >> ((s - 1) * m)) & layer_size_mask;
            // prv!(i, m, o);
            let o = i * 4;
            buf_u32.copy_from_slice(&data_block[o..o + 4]);
            let block = u32::from_le_bytes(buf_u32.clone()) as usize;
            // debug!("buf_u32: {:x?}, block: {:x}", buf_u32, block);
            if L != 1 {
                if L == 3 {
                    // if !self.walk_blocks::<2, F>(block, i, s + 1, &mut f)? {
                    if !self.walk_blocks::<2, F>(block, (i >> ((s - 1) * m)) + 12, s + 1, &mut f)? {
                        debug!("quit <2> on i={}", i);
                        return Ok(false);
                    };
                }
                if L == 2 {
                    // if !self.walk_blocks::<1, F>(block, i, s + 1, &mut f)? {
                    if !self.walk_blocks::<1, F>(block, (i >> ((s - 1) * m)) + 12, s + 1, &mut f)? {
                        debug!("quit <1> on i={}", i);
                        debug!("thresholds: 0={} 1={} 2={} 3={}", self.threshold(0),
                            self.threshold(1), self.threshold(2), self.threshold(3));
                        return Ok(false);
                    };
                }
            } else {
                debug!("call f(block={}, index={})", block, i);
                let r = f(block, i)?;
                if !r { return Ok(r); }
            }
        }
        Ok(true)
    }

    /// Walk for ino
    pub fn walk_blocks_inode<F>(self: &mut Self, ino: usize, block_index: usize, f: &mut F) -> Result<()>
        where F: FnMut(usize, usize) -> Result<bool> {
        let inode = self.get_inode(ino)?;
        macro_rules! visit_layer {
            ($l:expr) => {
                visit_layer_from!($l, self.threshold($l - 1));
            };
        }
        macro_rules! visit_layer_from {
            ($l:expr, $start:expr) => {
                if !self.walk_blocks::<$l, F>(inode.i_block[11 + $l] as usize, $start, 1, f)? { return Ok(()); };
            };
        }
        warn!("i_blocks[12, 13, 14] = {}, {}, {}", inode.i_block[12], inode.i_block[13], inode.i_block[14]);
        // if block_index < self.threshold(0) {
        for i in block_index..self.threshold(0) {
            if inode.i_block[i] == 0 || !f(inode.i_block[i] as usize, i)? { return Ok(()); }
        }
        // continue
        visit_layer!(1);
        visit_layer!(2);
        panic!("L3");
        visit_layer!(3);
        // } else if block_index < self.threshold(1) {
        //     // debug!("START from layer 1");
        //     visit_layer_from!(1, block_index);
        //     visit_layer!(2);
        //     visit_layer!(3);
        // } else if block_index < self.threshold(2) {
        //     error!("START from layer 2");
        //     // visit_layer_from!(2, block_index);
        //     visit_layer!(2);
        //     visit_layer!(3);
        // } else if block_index < self.threshold(3) {
        //     error!("START from layer 3");
        //     // visit_layer_from!(3, block_index);
        //     visit_layer!(3);
        // } else {
        //     return Err(anyhow!("Too big block_index!"));
        // }
        Ok(())
    }

    pub fn visit_blocks_inode<F>(self: &mut Self, ino: usize, block_index: usize, f: &mut F) -> Result<()>
        where F: FnMut(usize, usize) -> Result<(bool, bool)> {
        let mut inode = self.get_inode(ino)?;
        let mut inode_modified = false;
        macro_rules! save_inode_and_exit {
            ($modified:expr) => {
                if $modified { self.set_inode(ino, &inode)?; }
                return Ok(());
            };
            () => {
                save_inode_and_exit!(true);
            }
        }
        // macro_rules! call_f {
        //     ($block:expr, $index:expr, $inode_modified:expr) => {
        //         {
        //             let mut block = $block as usize;
        //             loop {
        //                 let r = f(block, $index)?;
        //                 if !r.1 {
        //                     // reach data end, and need to allocate new block
        //                     block = self.allocate_block()?;
        //                     $inode_modified = true;
        //                 } else {
        //                     if !r.0 { save_inode_and_exit!($inode_modified); }
        //                 }
        //             }
        //             block
        //         }
        //     };
        // }
        for i in block_index..self.threshold(0) {
            // call_f!()
            loop {
                let r = f(inode.i_block[i] as usize, i)?;
                if !r.1 {
                    // reach data end, and need to allocate new block
                    let new_block = self.allocate_block()?;
                    inode.i_block[i] = new_block as u32;
                    inode_modified = true;
                } else {
                    if !r.0 { save_inode_and_exit!(inode_modified); }
                    break;
                }
            }
        }
        let layer_size = self.block_size() / 4;
        let mut layer_index = [usize::MAX; 3];
        let mut layer_modified = [false; 3];
        let mut layer_data = vec![self.create_block_vec(); 3];
        let mut buf_u32 = [0 as u8; 4];
        macro_rules! dump_index_table {
            ($l:expr) => {
                if layer_modified[$l] {
                    self.write_data_block(layer_index[$l], &layer_data[$l])?;
                    layer_modified[$l] = false;
                }
            };
        }
        // 12 -> L1
        for i in max(block_index, self.threshold(0))..self.threshold(1) {
            let block_number = inode.i_block[12] as usize;
            if layer_index[0] != block_number {
                dump_index_table!(0);
                self.read_data_block(block_number, &mut layer_data[0])?;
                layer_index[0] = block_number;
            }
            let offset = (i - self.threshold(0)) << 2;
            loop {
                let layer_slice = &mut layer_data[0][offset..offset + 4];
                buf_u32.copy_from_slice(layer_slice);
                let block = u32::from_le_bytes(buf_u32.clone()) as usize;
                let r = f(block, i)?;
                if !r.1 {
                    let new_block = self.allocate_block()?;
                    layer_slice.copy_from_slice(&new_block.to_be_bytes());
                    layer_modified[0] = false;
                } else {
                    if !r.0 { save_inode_and_exit!(layer_modified[0]); }
                    break;
                }
            }
        }
        dump_index_table!(0);
        // 13 -> L2
        for i in range_step(self.threshold(1), self.threshold(2), layer_size) {
            let block_number = inode.i_block[13] as usize;
            if layer_index[0] != block_number {
                dump_index_table!(0);
                self.read_data_block(block_number, &mut layer_data[0])?;
                layer_index[0] = block_number;
            }
            let offset = ((i - self.threshold(1)) << 2) / layer_size;
            buf_u32.copy_from_slice(&layer_data[0][offset..offset + 4]);
            let block = u32::from_le_bytes(buf_u32.clone()) as usize;

            for j in i..i + layer_size {
                if block_index > j { continue; }
                let block_number = block;
                if layer_index[1] != block_number {
                    dump_index_table!(1);
                    self.read_data_block(block_number, &mut layer_data[1])?;
                    layer_index[1] = block_number;
                }
                let offset = ((j - 12) % layer_size) << 2;
                buf_u32.copy_from_slice(&layer_data[1][offset..offset + 4]);
                let block = u32::from_le_bytes(buf_u32.clone()) as usize;
                let r = f(block, j)?;
                if !r.0 { return Ok(()); }
            }
        }
        dump_index_table!(0);
        dump_index_table!(1);
        // 14 -> L3
        panic!("L3");
        // TODO: L3, bigger file will be not found
        debug!("L3 base block: {:x?}", inode.i_block);
        for i in range_step(self.threshold(2), self.threshold(3), layer_size * layer_size) {
            let block_number = inode.i_block[14] as usize;
            if layer_index[0] != block_number {
                self.read_data_block(block_number, &mut layer_data[0])?;
                layer_index[0] = block_number;
            }
            let offset = ((i - self.threshold(1)) << 2) / layer_size;
            buf_u32.copy_from_slice(&layer_data[0][offset..offset + 4]);
            let block = u32::from_le_bytes(buf_u32.clone()) as usize;

            for j in i..i + layer_size * layer_size {
                if block_index > j { continue; }
                let block_number = block;
                if layer_index[1] != block_number {
                    self.read_data_block(block_number, &mut layer_data[1])?;
                    layer_index[1] = block_number;
                }
                let offset = (((j - 12) % layer_size) / layer_size) << 2;
                buf_u32.copy_from_slice(&layer_data[1][offset..offset + 4]);
                let block = u32::from_le_bytes(buf_u32.clone()) as usize;

                for k in j..j + layer_size {
                    if block_index > k { continue; }
                    let block_number = block;
                    if layer_index[2] != block_number {
                        self.read_data_block(block_number, &mut layer_data[2])?;
                        layer_index[2] = block_number;
                    }
                    let offset = ((k - 12) % layer_size) << 2;
                    buf_u32.copy_from_slice(&layer_data[2][offset..offset + 4]);
                    let block = u32::from_le_bytes(buf_u32.clone()) as usize;

                    let r = f(block, k)?;
                    if !r.0 { return Ok(()); }
                }
            }
        }
        Ok(())
    }

    /// reserved for compatibility
    pub fn shift_ino(ino: u64) -> usize {
        // if ino == 1 { EXT2_ROOT_INO } else { ino as usize }
        // if ino == 1 { 0 } else { ino as usize }
        // (ino + 1) as usize
        // used for version 0
        ino as usize
    }

    pub fn bitmap_search(bitmap: &[u8]) -> Result<usize> {
        for (i, byte) in bitmap.iter().enumerate() {
            let b = *byte;
            for j in 0..8 {
                if (b >> j) & 0x1 == 0 {
                    // if b & (1 << j) == 0 {
                    // found free bit, return
                    // return Ok(i * 8 + j);
                    return Ok(i * 8 + j + 1);
                }
            }
        };
        Err(anyhow!("Bitmap full!"))
    }

    pub fn bitmap_set(bitmap: &mut [u8], index: usize) {
        let index = if index == 0 { 0 } else { index - 1 };
        let b = bitmap[index / 8] | (1 << (index % 8));
        bitmap[index / 8] = b;
    }

    pub fn make_node(&mut self, parent: usize, name: &str,
                     mode: usize, node_type: Ext2FileType) -> Result<(usize, Ext2INode)> {
        let file_type: usize = node_type.clone().into();
        let mut inode_parent = self.get_inode(parent as usize)?;
        // search inode bitmap for free inode
        let ino_free = self.allocate_inode()?;

        // create entry and inode
        // let mut entry = Ext2DirEntry::new_file(name, ino_free);
        let mut entry = Ext2DirEntry::new(name, ino_free, file_type as u8);
        let mut inode = Ext2INode::default();
        entry.inode = ino_free as u32;
        debug!("entry use new ino {}", ino_free);

        inode.i_mode = (mode & 0xFFF) as u16 | (file_type << 12) as u16;
        // append to parent dir entry
        let mut last_block_i = usize::MAX;
        for (i, d) in inode_parent.i_block.iter().enumerate() {
            if *d != 0 { last_block_i = i; }
        }
        if last_block_i == usize::MAX {
            panic!("data block is empty");
        }
        // TODO layer 1-3 support
        assert!(!(last_block_i == 12 || last_block_i == 13 || last_block_i == 14));
        // read parent dir
        let mut parent_enties = self.get_block_dir_entries(inode_parent.i_block[last_block_i] as usize)?;
        let mut entries_lengths: Vec<usize> = parent_enties.iter().map(|e| e.rec_len as usize).collect::<Vec<_>>();
        let mut offset_cnt = 0 as usize;
        let mut reset_last_rec_len = false;
        for (i, len) in entries_lengths.iter().enumerate() {
            if i != entries_lengths.len() - 1 {
                offset_cnt += len;
            } else {
                // last entry may have a large rec_len
                // calculate real size
                if *len as i64 - (8 + parent_enties[i].name_len) as i64 >= entry.rec_len as i64 {
                    reset_last_rec_len = true;
                    offset_cnt += parent_enties[i].name_len as usize + 8;
                } else {
                    offset_cnt += len;
                }
            }
        }
        prv!(entries_lengths, offset_cnt);
        if offset_cnt + entry.rec_len as usize >= self.block_size() {
            debug!("offset overflow! old last_block_i: {}, block: {}", last_block_i, inode_parent.i_block[last_block_i]);
            // append block index and load next block
            assert_ne!(last_block_i, 11);
            last_block_i += 1;
            let block_free = self.allocate_block()?;
            inode_parent.i_block[last_block_i] = block_free as u32;
            // reload parent_dir
            let parent_enties_2 = self.get_block_dir_entries(block_free)?;
            parent_enties = parent_enties_2;
            entries_lengths = parent_enties.iter().map(|e| e.rec_len as usize).collect();
            offset_cnt = 0;
            reset_last_rec_len = false;
            for len in entries_lengths { offset_cnt += len; }
        }
        debug!("parent inode blocks: {:x?}", inode_parent.i_block);
        debug!("write entry to buf, rec_len = {}", entry.rec_len);
        let mut data_block = self.create_block_vec();
        // self.read_block(&mut data_block)?;
        self.read_data_block(inode_parent.i_block[last_block_i] as usize, &mut data_block)?;
        debug!("original data_block:");
        show_hex_debug(&data_block[..0x50], 0x10);
        if reset_last_rec_len {
            debug!("write back modified parent entries");
            let parent_entries_tail = parent_enties.len() - 1;
            let mut parent_entries_last = parent_enties[parent_entries_tail].clone();
            debug!("parent_entries_last: {} {:?}", parent_entries_last.to_string(), parent_entries_last);
            let parent_entries_last_rec_len_old = parent_entries_last.rec_len as usize;
            let offset_start = self.block_size() - parent_entries_last_rec_len_old;
            parent_entries_last.rec_len = (up_align((parent_entries_last.name_len + 8) as usize, 4)) as u16;
            debug!("parent_entries_last updated: {} {:?}", parent_entries_last.to_string(), parent_entries_last);
            let parent_entries_last_data = unsafe { serialize_row(&parent_entries_last) };
            // data_block[offset_cnt - parent_entries_last_rec_len_old as usize..offset_cnt + (parent_entries_last.rec_len - parent_entries_last_rec_len_old) as usize]
            //     .copy_from_slice(&parent_entries_last_data[..parent_entries_last.rec_len as usize]);
            let offset_next = offset_start + parent_entries_last.rec_len as usize;
            debug!("data_block update: [{:x}..{:x}]", offset_start, offset_next);
            data_block[offset_start..offset_next]
                .copy_from_slice(&parent_entries_last_data[..parent_entries_last.rec_len as usize]);
            // offset_cnt = up_align(offset_cnt, 4);
            debug!("data_block after update:");
            show_hex_debug(&data_block[..0x50], 0x10);
            offset_cnt = offset_next;
        }
        let old_rec_len = entry.rec_len;
        let offset_next = offset_cnt + old_rec_len as usize;
        entry.rec_len = (self.block_size() - offset_cnt) as u16;
        debug!("new entry to write: {} {:?}", entry.to_string(), entry);
        let entry_data = unsafe { serialize_row(&entry) };
        debug!("update data_block for entry_data: [{:x}..{:x}]", offset_cnt, offset_next);
        data_block[offset_cnt..offset_next].copy_from_slice(&entry_data[..old_rec_len as usize]);
        debug!("data_block to write:");
        show_hex_debug(&data_block[..0x50], 0x10);
        debug!("write back buf block: {}", inode_parent.i_block[last_block_i]);
        self.write_data_block(inode_parent.i_block[last_block_i] as usize, &data_block)?;
        let attr = inode.to_attr(ino_free);
        debug!("file {} == {} created! attr: {:?}", name, entry.get_name(), attr);

        let mut data_block_free = 0 as usize;
        match node_type {
            Ext2FileType::Directory | Ext2FileType::RegularFile => {
                debug!("finding new data block...");
                let block_free = self.allocate_block()?;
                data_block_free = block_free;
                debug!("found free block: {}", data_block_free);
            }
            _ => {}
        };
        match node_type {
            Ext2FileType::Directory => {
                debug!("is directory, creating directory entries at block {}", data_block_free);
                debug!("set self size to one block...");
                inode.i_blocks = 1;
                inode.i_size = self.block_size() as u32;
                debug!("inode.i_blocks = {}, inode.i_size = {}", inode.i_blocks, inode.i_size);
                let mut entries = vec![];
                debug!("set first data block...");
                inode.i_block[0] = data_block_free as u32;
                debug!("data block now: {:?}", inode.i_block[0]);
                let mut dir_this = entry.clone();
                dir_this.update_name(".");
                entries.push(dir_this);
                let dir_parent = Ext2DirEntry::new_dir("..", parent);
                entries.push(dir_parent);
                let lens = entries.iter().map(|e| e.rec_len as usize).collect::<Vec<_>>();
                let mut offset = 0;
                for l in &lens {
                    offset += l;
                }
                // pad rec_len
                let mut entry_last = entries.last().unwrap().clone();
                offset -= entry_last.rec_len as usize;
                let entry_last_real_len = entry_last.rec_len;
                entry_last.rec_len = (self.block_size() - offset) as u16;
                let entries_len = entries.len();
                entries[entries_len - 1] = entry_last;
                let mut block_data = self.create_block_vec();
                offset = 0;
                for (i, entry) in entries.iter().enumerate() {
                    debug!("write directory entry {} {:?}", entry.to_string(), entry);
                    if i != entries_len - 1 {
                        debug!("write offset [{}..{}]", offset, offset + entry.rec_len as usize);
                        block_data[offset..offset + entry.rec_len as usize]
                            .copy_from_slice(&unsafe { serialize_row(entry) }[..entry.rec_len as usize]);
                        offset += entry.rec_len as usize;
                    } else {
                        debug!("write offset [{}..{}]", offset, offset + entry_last_real_len as usize);
                        block_data[offset..offset + entry_last_real_len as usize]
                            .copy_from_slice(&unsafe { serialize_row(entry) }[..entry_last_real_len as usize]);
                    }
                }
                self.write_data_block(data_block_free, &block_data)?;
            }
            Ext2FileType::RegularFile => {
                debug!("is regular file, first data at block {}", data_block_free);
            }
            _ => {}
        };


        debug!("write new inode: [{}] {:?}", ino_free, inode);
        self.set_inode(ino_free, &inode)?;
        debug!("write parent inode: [{}] {:?}", parent, inode_parent);
        self.set_inode(parent as usize, &inode_parent)?;
        Ok((ino_free, inode))
    }

    fn allocate_bitmap(&mut self, bitmap_block: usize, is_data: bool) -> Result<usize> {
        let bitmap = if is_data { &mut self.bitmap_data } else { &mut self.bitmap_inode };
        let block_free = Self::bitmap_search(bitmap)?;
        Self::bitmap_set(bitmap, block_free);
        // save bitmap
        let bitmap_clone: Vec<u8> = bitmap.clone();
        self.write_data_block(bitmap_block, &bitmap_clone)?;
        Ok(block_free)
    }

    pub fn allocate_block(&mut self) -> Result<usize> {
        let block = self.get_group_desc().bg_block_bitmap as usize;
        self.allocate_bitmap(block, true)
    }

    pub fn allocate_inode(&mut self) -> Result<usize> {
        let block = self.get_group_desc().bg_inode_bitmap as usize;
        self.allocate_bitmap(block, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
