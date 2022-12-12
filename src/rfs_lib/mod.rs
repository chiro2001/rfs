/// Filesystem logics
use std::cmp::{max, min};
use std::fs::File;
use std::io::Read;
use std::mem::size_of;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
pub use disk_driver;
use anyhow::{anyhow, Result};
use disk_driver::{DiskDriver, DiskInfo, IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE, SeekType};
use execute::Execute;
use log::*;
use num::range_step;
// use macro_tools::*;

#[macro_use]
pub mod utils;
pub mod desc;
pub mod types;
pub mod mem;
pub mod fuse;

use utils::*;
use mem::*;
use desc::*;
use crate::{DEVICE_FILE, FORCE_FORMAT, LAYOUT_FILE, MKFS_FORMAT, prv};

/// Data TTL, 1 second default
const TTL: Duration = Duration::from_secs(1);

#[derive(Default, Clone)]
pub struct RFSBase {
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

impl RFSBase {
    #[allow(dead_code)]
    pub fn set(&mut self, d: Self) {
        self.driver_info = d.driver_info;
        self.super_block = d.super_block;
        self.group_desc_table = d.group_desc_table;
        self.filesystem_first_block = d.filesystem_first_block;
        self.bitmap_inode = d.bitmap_inode;
        self.bitmap_data = d.bitmap_data;
        self.root_dir = d.root_dir;
    }
}

// #[derive(ApplyMemType, Default)]
// #[ApplyMemTo(RFSBase)]
// #[ApplyMemType(T)]
pub struct RFS<T: DiskDriver> {
    pub driver: T,
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

impl<T: DiskDriver> Into<RFSBase> for RFS<T> {
    fn into(self) -> RFSBase {
        RFSBase {
            driver_info: self.driver_info,
            super_block: self.super_block,
            group_desc_table: self.group_desc_table,
            filesystem_first_block: self.filesystem_first_block,
            bitmap_inode: self.bitmap_inode,
            bitmap_data: self.bitmap_data,
            root_dir: self.root_dir,
        }
    }
}

impl<T: DiskDriver> RFS<T> {
    /// Create RFS object from selected DiskDriver
    #[allow(dead_code)]
    pub fn new(driver: T) -> Self {
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

    #[allow(dead_code)]
    pub fn from_base(that: RFSBase, driver: T) -> Self {
        Self {
            driver,
            driver_info: that.driver_info,
            super_block: that.super_block,
            group_desc_table: that.group_desc_table,
            filesystem_first_block: that.filesystem_first_block,
            bitmap_inode: that.bitmap_inode,
            bitmap_data: that.bitmap_data,
            root_dir: that.root_dir,
        }
    }

    /// Get disk unit, available after init
    fn disk_block_size(&self) -> usize { self.driver_info.consts.iounit_size as usize }

    /// Get disk sizs, available after init
    fn disk_size(&self) -> usize { self.driver_info.consts.layout_size as usize }

    /// Get filesystem block size, available after init
    pub fn block_size(&self) -> usize { (1 << self.super_block.s_log_block_size) * 0x400 as usize }

    pub fn get_driver(&mut self) -> &mut T {
        &mut self.driver
    }

    /// Read one disk block
    fn read_disk_block(&mut self, buf: &mut [u8]) -> Result<()> {
        assert_eq!(buf.len(), self.disk_block_size());
        let sz = self.disk_block_size();
        self.get_driver().ddriver_read(buf, sz)?;
        Ok(())
    }

    /// Write one disk block
    fn write_disk_block(&mut self, buf: &[u8]) -> Result<()> {
        assert_eq!(buf.len(), self.disk_block_size());
        let sz = self.disk_block_size();
        self.get_driver().ddriver_write(buf, sz)?;
        Ok(())
    }

    /// Read multi disk units from disk
    fn read_disk_blocks(&mut self, buf: &mut [u8], count: usize) -> Result<()> {
        let sz = self.disk_block_size();
        for i in 0..count { self.read_disk_block(&mut buf[(i * sz)..((i + 1) * sz)])? }
        Ok(())
    }

    /// Write multi disk units from disk
    fn write_disk_blocks(&mut self, buf: &[u8], count: usize) -> Result<()> {
        let sz = self.disk_block_size();
        for i in 0..count { self.write_disk_block(&buf[(i * sz)..((i + 1) * sz)])? }
        Ok(())
    }

    /// Seek disk cursor by bytes
    fn seek_disk_block(&mut self, index: usize) -> Result<()> {
        let sz = self.disk_block_size();
        // info!("DISK seek to {:x}", index * sz);
        let _n = self.get_driver().ddriver_seek((index * sz) as i64, SeekType::Set)?;
        Ok(())
    }

    /// How many disk unit for one filesystem block.
    /// fs block size should larger than ont disk unit
    fn block_disk_ratio(&self) -> usize { self.block_size() / self.disk_block_size() }

    /// Seek disk by unit of fs block size
    pub fn seek_block(&mut self, index: usize) -> Result<()> {
        self.seek_disk_block(index * self.block_disk_ratio())
    }

    /// Read disk by one block
    pub fn read_block(&mut self, buf: &mut [u8]) -> Result<()> {
        self.read_disk_blocks(buf, self.block_disk_ratio())
    }

    /// Write disk by one block
    pub fn write_block(&mut self, buf: &[u8]) -> Result<()> {
        self.write_disk_blocks(buf, self.block_disk_ratio())
    }

    /// Read disk by multi-blocks
    #[allow(dead_code)]
    pub fn read_blocks(&mut self, buf: &mut [u8], count: usize) -> Result<()> {
        self.read_disk_blocks(buf, self.block_disk_ratio() * count)
    }

    /// Write disk by multi-blocks
    #[allow(dead_code)]
    pub fn write_blocks(&mut self, buf: &[u8], count: usize) -> Result<()> {
        self.write_disk_blocks(buf, self.block_disk_ratio() * count)
    }

    /// Create a Vec<u8> in block size
    pub fn create_block_vec(&mut self) -> Vec<u8> {
        [0 as u8].repeat(self.block_size())
    }

    /// Create a Vec<u8> in multi-blocks size
    #[allow(dead_code)]
    pub fn create_blocks_vec(&self, count: usize) -> Vec<u8> {
        [0 as u8].repeat(self.block_size() * count)
    }

    /// Get `Ext2GroupDesc`, available after init
    fn get_group_desc(&self) -> &Ext2GroupDesc {
        self.group_desc_table.get(0).unwrap()
    }

    /// Print basic fs info
    /// see: https://lostjeffle.bitcron.com/blog/MWeb/docs/media/15901301484642/15247422226670.jpg
    pub fn print_stats(&self) {
        info!("fs stats: {}", self.super_block.to_string());
        info!("fs layout:");
        println!("| BSIZE = {} B |", self.block_size());
        let mut block_layout: Vec<String> = vec![];
        block_layout.push("Boot(1)".to_string());
        block_layout.push("Super(1)".to_string());
        block_layout.push("GroupDesc(1)".to_string());
        block_layout.push("DATA Map(1)".to_string());
        block_layout.push("Inode Map(1)".to_string());
        block_layout.push(format!("Inode Table({})", self.super_block.s_inodes_count as usize
            / (self.block_size() / size_of::<Ext2INode>())));
        block_layout.push("DATA(*)".to_string());
        println!("| {} |", block_layout.join(" | "));
        info!("For inode bitmap, see @ {:x}", self.get_group_desc().bg_inode_bitmap as usize * self.block_size());
        info!("For  data bitmap, see @ {:x}", self.get_group_desc().bg_block_bitmap as usize * self.block_size());
    }

    /// Calculate block number and offset in a block for inode
    fn fetch_inode_block_offset(&self, ino: usize) -> Result<(usize, usize)> {
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
    pub fn get_inode(&mut self, ino: usize) -> Result<Ext2INode> {
        let (block_number, offset) = self.fetch_inode_block_offset(ino)?;
        debug!("get_inode: inode {} at block {} offset {:x}, disk offset is {:x}",
            ino, block_number, offset, block_number * self.block_size());
        let mut buf = self.create_block_vec();
        self.seek_block(block_number)?;
        self.read_block(&mut buf)?;
        Ok(unsafe { deserialize_row(&buf[offset..]) })
    }

    /// Write inode struct according to ino number
    pub fn set_inode(&mut self, ino: usize, inode: &Ext2INode) -> Result<()> {
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
    pub fn get_data_block(&mut self, block: usize) -> Result<Vec<u8>> {
        self.seek_block(block)?;
        let mut buf = self.create_block_vec();
        self.read_block(&mut buf)?;
        Ok(buf)
    }

    /// Read one data block to mutable slice inplace
    pub fn read_data_block(&mut self, block: usize, buf: &mut [u8]) -> Result<()> {
        self.seek_block(block)?;
        self.read_block(buf)?;
        Ok(())
    }

    /// Write one data block from slice inplace
    pub fn write_data_block(&mut self, block: usize, buf: &[u8]) -> Result<()> {
        self.seek_block(block)?;
        assert!(buf.len() <= self.block_size(), "support sz <= block");
        if buf.len() % self.block_size() == 0 {
            self.write_block(buf)?;
        } else {
            // debug!("write part of one block, read and update; source buf:");
            // show_hex_debug(buf, 16);
            let mut block_data = self.create_block_vec();
            self.read_data_block(block, &mut block_data)?;
            block_data[..buf.len()].copy_from_slice(buf);
            self.write_data_block(block, &mut block_data)?;
        }
        Ok(())
    }

    /// Read all directory entries in one block
    pub fn get_block_dir_entries(&mut self, block: usize) -> Result<Vec<Ext2DirEntry>> {
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
    pub fn get_dir_entries(&mut self, ino: usize) -> Result<Vec<Ext2DirEntry>> {
        let inode = self.get_inode(ino)?;
        if inode.i_mode as usize >> 12 != Ext2FileType::Directory.into() {
            return Err(anyhow!("ino {} is not a directory!", ino));
        }
        prv!(inode);
        // TODO: walk all blocks, including indirect blocks
        // let offset = offset as usize;
        // let size = size as usize;
        // let sz = self.block_size();
        // let ino = RFS::<T>::shift_ino(ino);
        //
        // let mut blocks: Vec<usize> = vec![];
        //
        // rep!(reply, self.walk_blocks_inode(ino, offset / self.block_size(), &mut |block, index| {
        //     debug!("walk to block {} index {}", block, index);
        //     blocks.push(block);
        //     Ok(index * sz < size)
        // }));

        // TODO: layer 1-3 directory entries supporting
        Ok(inode.i_block.iter().take(12)
            .map(|b| match self.get_block_dir_entries(*b as usize) {
                Ok(e) => e,
                Err(_) => vec![]
            }).into_iter()
            .filter(|x| !x.is_empty()).flatten().collect())
    }

    /// Block index layer threshold
    pub fn threshold(&self, l: usize) -> usize {
        let layer = self.block_size() / 4;
        match l {
            0 => 12,
            1 => 12 + layer,
            2 => 12 + layer + layer * layer,
            3 => 11 + layer + layer * 2 + layer * layer,
            _ => panic!("Walk layer out of range")
        }
    }

    #[allow(dead_code)]
    pub fn threshold_diff(&self, l: usize) -> usize {
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
    #[allow(dead_code)]
    pub fn walk_blocks<const L: usize, F>(&mut self, start_block: usize, block_index: usize, s: usize, mut f: &mut F) -> Result<bool>
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
            let block = u32::from_be_bytes(buf_u32.clone()) as usize;
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
    #[allow(dead_code)]
    pub fn walk_blocks_inode<F>(&mut self, ino: usize, block_index: usize, f: &mut F) -> Result<()>
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
        debug!("i_blocks[12, 13, 14] = {}, {}, {}", inode.i_block[12], inode.i_block[13], inode.i_block[14]);
        // if block_index < self.threshold(0) {
        for i in block_index..self.threshold(0) {
            if inode.i_block[i] == 0 || !f(inode.i_block[i] as usize, i)? { return Ok(()); }
        }
        // continue
        visit_layer!(1);
        visit_layer!(2);
        // panic!("L3");
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

    pub fn visit_blocks_inode<F>(&mut self, ino: usize, block_index: usize, f: &mut F) -> Result<()>
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
        for i in block_index..self.threshold(0) {
            loop {
                let r = f(inode.i_block[i] as usize, i)?;
                if r.1 {
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
                self.set_inode(ino, &inode)?;
                debug!("modified: {}, layer_index[{}]: {}", layer_modified[$l], $l, layer_index[$l]);
                if layer_modified[$l] && layer_index[$l] != 0 && layer_index[$l] != usize::MAX {
                    self.write_data_block(layer_index[$l], &layer_data[$l])?;
                    layer_modified[$l] = false;
                }
            };
        }
        // 12 -> L1
        for i in max(block_index, self.threshold(0))..self.threshold(1) {
            let base_block_number = inode.i_block[12];
            if base_block_number == 0 {
                // alloc block for layer index data
                let new_layer_block = self.allocate_block()?;
                inode.i_block[12] = new_layer_block as u32;
                debug!("new_block for layer index block: {}", new_layer_block);
                // clear data
                let layer_index_data = self.create_block_vec();
                self.write_data_block(new_layer_block, &layer_index_data)?;
                self.read_data_block(base_block_number as usize, &mut layer_data[0])?;
                layer_index[0] = base_block_number as usize;
            }
            loop {
                let block_number = inode.i_block[12] as usize;
                if layer_index[0] != block_number && block_number != 0 {
                    debug!("L1: saving layer index data at block {}", layer_index[0]);
                    dump_index_table!(0);
                    debug!("L1: getting layer index data for new block {}", block_number);
                    self.read_data_block(block_number, &mut layer_data[0])?;
                    layer_index[0] = block_number;
                }
                let offset = (i - self.threshold(0)) << 2;

                let layer_slice = &mut layer_data[0][offset..offset + 4];
                buf_u32.copy_from_slice(layer_slice);
                let block = u32::from_be_bytes(buf_u32.clone()) as usize;
                let r = f(block, i)?;
                if r.1 {
                    let new_block = self.allocate_block()? as u32;
                    layer_slice.copy_from_slice(&new_block.to_be_bytes());
                    layer_modified[0] = true;
                } else {
                    if !r.0 {
                        dump_index_table!(0);
                        save_inode_and_exit!(layer_modified[0]);
                    }
                    break;
                }
            }
        }
        if layer_modified[0] {
            debug!("L1: saving layer index data at block {}", layer_index[0]);
        }
        dump_index_table!(0);
        // 13 -> L2
        warn!("L2!");
        for i in max(block_index, self.threshold(1))..self.threshold(2) {
            let base_block_number = inode.i_block[13];
            if base_block_number == 0 {
                // alloc block for layer index data
                let new_layer_block = self.allocate_block()?;
                inode.i_block[13] = new_layer_block as u32;
                debug!("new_block for layer index block: {}", new_layer_block);
                // clear data
                let layer_index_data = self.create_block_vec();
                self.write_data_block(new_layer_block, &layer_index_data)?;
                self.read_data_block(base_block_number as usize, &mut layer_data[0])?;
                layer_index[0] = base_block_number as usize;
            }
            // let base_block_number = inode.i_block[13];
            loop {
                let block_number = inode.i_block[13] as usize;
                if layer_index[0] != block_number && block_number != 0 {
                    debug!("L2.0: saving layer index data at block {}", layer_index[0]);
                    dump_index_table!(0);
                    debug!("L2.0: getting layer index data for new block {}", block_number);
                    self.read_data_block(block_number, &mut layer_data[0])?;
                    layer_index[0] = block_number;
                }

                let offset = ((i - self.threshold(1)) / layer_size) << 2;
                let layer_slice = &mut layer_data[0][offset..offset + 4];
                buf_u32.copy_from_slice(layer_slice);
                let block_number2 = u32::from_be_bytes(buf_u32.clone()) as usize;
                if layer_index[1] != block_number2 && block_number2 != 0 {
                    debug!("L2.1: saving layer index data at block {}", layer_index[1]);
                    dump_index_table!(1);
                    debug!("L2.1: getting layer index data for new block {}", block_number2);
                    self.read_data_block(block_number2, &mut layer_data[1])?;
                    layer_index[1] = block_number2;
                }

                let offset2 = ((i - 12) % layer_size) << 2;
                let layer_slice2 = &mut layer_data[1][offset2..offset2 + 4];
                buf_u32.copy_from_slice(layer_slice2);
                let block2 = u32::from_be_bytes(buf_u32.clone()) as usize;
                debug!("ldata[0][{}..+4] = {}, ldata[1][{}..+4] = {}", offset, block_number2, offset2, block2);

                let r = f(block2, i)?;
                if r.1 {
                    if block_number2 == 0 {
                        let new_block = self.allocate_block()? as u32;
                        debug!("full, allocate on layer 1, new block: {}, offset: {}", new_block, offset);
                        let layer_index_data = self.create_block_vec();
                        self.write_data_block(new_block as usize, &layer_index_data)?;
                        layer_data[0][offset..offset + 4].copy_from_slice(&new_block.to_be_bytes());
                        layer_modified[0] = true;
                        self.read_data_block(new_block as usize, &mut layer_data[1])?;
                        layer_index[1] = new_block as usize;
                    }
                    let new_block = self.allocate_block()? as u32;
                    layer_data[1][offset2..offset2 + 4].copy_from_slice(&new_block.to_be_bytes());
                    layer_modified[1] = true;
                } else {
                    if !r.0 {
                        dump_index_table!(0);
                        dump_index_table!(1);
                        // save_inode_and_exit!(layer_modified[0]);
                        if layer_modified[0] { self.set_inode(ino, &inode)?; }
                        save_inode_and_exit!(layer_modified[1]);
                    }
                    break;
                }
            }
        }
        dump_index_table!(0);
        dump_index_table!(1);
        // 14 -> L3
        // panic!("L3");
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
            let block = u32::from_be_bytes(buf_u32.clone()) as usize;

            for j in i..i + layer_size * layer_size {
                if block_index > j { continue; }
                let block_number = block;
                if layer_index[1] != block_number {
                    self.read_data_block(block_number, &mut layer_data[1])?;
                    layer_index[1] = block_number;
                }
                let offset = (((j - 12) % layer_size) / layer_size) << 2;
                buf_u32.copy_from_slice(&layer_data[1][offset..offset + 4]);
                let block = u32::from_be_bytes(buf_u32.clone()) as usize;

                for k in j..j + layer_size {
                    if block_index > k { continue; }
                    let block_number = block;
                    if layer_index[2] != block_number {
                        self.read_data_block(block_number, &mut layer_data[2])?;
                        layer_index[2] = block_number;
                    }
                    let offset = ((k - 12) % layer_size) << 2;
                    buf_u32.copy_from_slice(&layer_data[2][offset..offset + 4]);
                    let block = u32::from_be_bytes(buf_u32.clone()) as usize;

                    let r = f(block, k)?;
                    if !r.0 { return Ok(()); }
                }
            }
        }
        Ok(())
    }

    /// reserved for compatibility
    // pub fn shift_ino(ino: u64) -> usize {
    pub fn shift_ino(ino: usize) -> usize {
        // if ino == 1 { EXT2_ROOT_INO } else { ino as usize }
        // if ino == 1 { 0 } else { ino as usize }
        // (ino + 1) as usize
        // used for version 0
        ino as usize
    }

    pub fn bitmap_search(bitmap: &[u8], reserved: usize) -> Result<usize> {
        for (i, byte) in bitmap.iter().enumerate().skip(reserved) {
            let b = *byte;
            for j in 0..8 {
                if (b >> j) & 0x1 == 0 {
                    // found free bit, return
                    return Ok(i * 8 + j + 1);
                }
            }
        };
        Err(anyhow!("Bitmap full!"))
    }

    pub fn bitmap_set(bitmap: &mut [u8], index: usize) {
        debug!("setting bitmap for index {}", index);
        let index = if index == 0 { 0 } else { index - 1 };
        let b = bitmap[index / 8] | (1 << (index % 8));
        bitmap[index / 8] = b;
    }

    pub fn make_node(&mut self, parent: usize, name: &str,
                     mode: usize, node_type: Ext2FileType) -> Result<(usize, Ext2INode)> {
        debug!("make_node(parent={}, name={})", parent, name);
        let file_type: usize = node_type.clone().into();
        let mut inode_parent = self.get_inode(parent as usize)?;
        // search inode bitmap for free inode
        let ino_free = self.allocate_inode()?;

        // create entry and inode
        let mut entry = Ext2DirEntry::new(name, ino_free, file_type as u8);
        let mut inode = Ext2INode::default();
        entry.inode = ino_free as u32;
        debug!("entry use new ino {}", ino_free);

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

        inode.i_mode = (mode & 0xFFF) as u16 | (file_type << 12) as u16;
        // append to parent dir entry
        let mut last_block_i = usize::MAX;
        for (i, d) in inode_parent.i_block.iter().enumerate() {
            if *d != 0 { last_block_i = i; }
        }
        let mut init_directory_done = false;
        let block_size = self.block_size();
        let init_directory = |entry: &mut Ext2DirEntry, inode: &Ext2INode, data_block_free: usize|
                              -> Result<(Vec<u8>, Ext2INode)> {
            debug!("init_directory for ino {}", ino_free);
            let mut inode = inode.clone();
            inode.i_blocks = 1;
            inode.i_size = block_size as u32;
            debug!("inode.i_blocks = {}, inode.i_size = {}", inode.i_blocks, inode.i_size);
            let mut entries = vec![];
            debug!("set first data block...");
            inode.i_block[0] = data_block_free as u32;
            debug!("data block now: {:?}", inode.i_block[0]);
            let mut dir_this = entry.clone();
            if dir_this.name[0] == u8::try_from('.')? && dir_this.name_len == 1 {
                debug!("this_dir is '.', ignore inserting another '.'");
            } else {
                debug!("dir_this: {}", dir_this.to_string());
                dir_this.update_name(".");
                debug!("dir_this updated: {}", dir_this.to_string());
                entries.push(dir_this);
            }
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
            entry_last.rec_len = (block_size - offset) as u16;
            let entries_len = entries.len();
            entries[entries_len - 1] = entry_last;
            // let mut block_data = self.create_block_vec();
            let mut block_data = [0 as u8].repeat(block_size);
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
            // self.write_data_block(data_block_free, &block_data)
            Ok((block_data, inode))
        };
        if last_block_i == usize::MAX {
            if node_type == Ext2FileType::Directory {
                debug!("parent inode data block is empty, creating a block for this directory");
                let parent_data_block_free = self.allocate_block()?;
                if inode_parent.i_blocks == 0 && inode_parent.i_size == 0 && inode_parent.i_mode == 0 {
                    debug!("inode_parent is empty, use default value");
                    inode_parent = Ext2INode::default();
                    inode_parent.i_size = block_size as u32;
                    inode_parent.i_blocks = 1;
                    inode_parent.i_mode = (mode | (file_type << 12)) as u16;
                }
                let dir_entry_block_data = init_directory(&mut entry, &inode_parent, parent_data_block_free)?;
                init_directory_done = true;
                inode_parent = dir_entry_block_data.1;
                debug!("after update, inode parent: {:?}", inode_parent);
                self.write_data_block(parent_data_block_free, &dir_entry_block_data.0)?;
                last_block_i = 0;
            } else {
                panic!("data block is empty");
            }
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

        match node_type {
            Ext2FileType::Directory => {
                debug!("is directory, creating directory entries at block {}", data_block_free);
                if init_directory_done {
                    debug!("has init done, ignore creating directory entries")
                } else {
                    debug!("set self size to one block...");
                    let dir_entry_block_data = init_directory(&mut entry, &inode, data_block_free)?;
                    inode = dir_entry_block_data.1;
                    debug!("after update, inode: {:?}; ready to write block: {}", inode, data_block_free);
                    self.write_data_block(data_block_free, &dir_entry_block_data.0)?;
                }
            }
            Ext2FileType::RegularFile => {
                debug!("is regular file, first data at block {}", data_block_free);
                inode.i_block[0] = data_block_free as u32;
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
        let reserved_blocks = 1 + 1 + 1 + 1 + 1 + self.super_block.s_inodes_count as usize / size_of::<Ext2INode>() + 1;
        let block_free = Self::bitmap_search(bitmap, if is_data {
            reserved_blocks
        } else { self.super_block.s_first_ino as usize + 1 })?;
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

    pub fn rfs_init(&mut self, file: &str) -> Result<()> {
        self.get_driver().ddriver_open(file)?;
        // get and check size
        let mut buf = [0 as u8; 4];
        self.get_driver().ddriver_ioctl(IOC_REQ_DEVICE_SIZE, &mut buf)?;
        self.driver_info.consts.layout_size = u32::from_le_bytes(buf.clone());
        info!("disk layout size: {}", self.driver_info.consts.layout_size);
        self.get_driver().ddriver_ioctl(IOC_REQ_DEVICE_IO_SZ, &mut buf)?;
        self.driver_info.consts.iounit_size = u32::from_le_bytes(buf.clone());
        info!("disk unit size: {}", self.driver_info.consts.iounit_size);
        debug!("size of super block struct is {}", size_of::<Ext2SuperBlock>());
        debug!("size of group desc struct is {}", size_of::<Ext2GroupDesc>());
        debug!("size of inode struct is {}", size_of::<Ext2INode>());

        // at lease 32 blocks
        info!("Disk {} has {} IO blocks.", file, self.driver_info.consts.disk_block_count());
        if self.disk_size() < 32 * 0x400 {
            return Err(anyhow!("Too small disk! disk size is 0x{:x}", self.disk_size()));
        }
        info!("disk info: {:?}", self.driver_info);
        // read super block
        let super_blk_count = size_of::<Ext2SuperBlock>() / self.disk_block_size();
        let disk_block_size = self.disk_block_size();
        info!("super block size {} disk block ({} bytes)", super_blk_count, super_blk_count * self.disk_block_size());
        let mut data_blocks_head = [0 as u8].repeat((disk_block_size * super_blk_count) as usize);
        self.read_disk_blocks(&mut data_blocks_head, super_blk_count)?;
        let mut super_block: Ext2SuperBlock = unsafe { deserialize_row(&data_blocks_head) };
        if !super_block.magic_matched() {
            // maybe there is one block reserved for boot,
            // read one block again
            self.read_disk_blocks(&mut data_blocks_head, super_blk_count)?;
            // data_blocks_head.reverse();
            super_block = unsafe { deserialize_row(&data_blocks_head) };
            if super_block.magic_matched() { self.filesystem_first_block = 1; }
        }
        let format = FORCE_FORMAT.read().unwrap().clone();
        if !super_block.magic_matched() || format {
            if !format { warn!("FileSystem not found! creating super block..."); } else { warn!("Will format disk!") }
            let mkfs = MKFS_FORMAT.read().unwrap().clone();
            if mkfs {
                // let's use mkfs.ext2
                debug!("close driver");
                self.get_driver().ddriver_close()?;
                // create file
                let mut command = execute::command_args!("dd", format!("of={}", file), "if=/dev/zero",
                format!("bs={}", self.disk_block_size()),
                format!("count={}", self.disk_size() / self.disk_block_size()));
                command.stdout(Stdio::piped());
                let output = command.execute_output().unwrap();
                info!("{}", String::from_utf8(output.stdout).unwrap());
                // use version 0
                let mut command = execute::command_args!("mkfs.ext2", file, "-t", "ext2", "-r", "0");
                command.stdout(Stdio::piped());
                let output = command.execute_output().unwrap();
                info!("{}", String::from_utf8(output.stdout).unwrap());
                // reload disk driver
                self.get_driver().ddriver_open(&file)?;
                self.seek_block(0)?;
                self.read_disk_blocks(&mut data_blocks_head, super_blk_count)?;
                super_block = unsafe { deserialize_row(&data_blocks_head) };
                if !super_block.magic_matched() {
                    self.read_disk_blocks(&mut data_blocks_head, super_blk_count)?;
                    super_block = unsafe { deserialize_row(&data_blocks_head) };
                }
                if super_block.magic_matched() {
                    self.filesystem_first_block = 1;
                    info!("Disk driver reloaded.");
                } else {
                    return Err(anyhow!("Make filesystem failed!"));
                }
            } else {
                // use manual fs layout
                // reload disk driver
                self.seek_block(0)?;
                let default_layout_str = "
| BSIZE = 1024 B |
| Boot(1) | Super(1) | GroupDesc(1) | DATA Map(1) | Inode Map(1) | Inode Table(128) | DATA(*) |";
                let layout_file = LAYOUT_FILE.read().unwrap().clone();
                debug!("loading {}...", layout_file);
                let path = Path::new(&layout_file);
                let mut layout_string = default_layout_str.to_string();
                if path.exists() {
                    let mut file = File::open(path).unwrap();
                    let mut data = vec![];
                    file.read_to_end(&mut data).unwrap();
                    layout_string = String::from_utf8(data).unwrap();
                } else {
                    warn!("{}({}) not found! use default layout: {}", layout_file, path.to_str().unwrap(), default_layout_str);
                }
                let lines = layout_string.lines();
                let mut layout = FsLayoutArgs::default();
                for line in lines {
                    if line.is_empty() || !line.starts_with("|") { continue; }
                    let line = line.to_lowercase();
                    if line.contains("bsize") {
                        let splits = line.split(" ").collect::<Vec<&str>>();
                        // debug!("split = {:?}", splits);
                        let n = splits[3];
                        // debug!("split n = {}", n);
                        layout.block_size = str::parse::<usize>(n).unwrap();
                        info!("block_size = {}", layout.block_size);
                    } else {
                        let splits = line.split("|")
                            .map(|x| x.trim())
                            .filter(|x| x.len() > 0)
                            .filter(|x| x.contains("("))
                            .collect::<Vec<&str>>();
                        debug!("splits: {:?}", splits);
                        let mut offset_block = 0;
                        for s in splits {
                            let v = if s.contains("*") {
                                0 as usize
                            } else {
                                str::parse::<usize>(&s[s.find("(").unwrap() + 1..s.len() - 1]).unwrap()
                            };
                            let name = &s[..s.find("(").unwrap()];
                            debug!("{} = {}", name, v);
                            match name {
                                "boot" => {
                                    layout.boot = true;
                                    offset_block += 1;
                                }
                                "super" => {
                                    layout.super_block = offset_block;
                                    offset_block += v;
                                }
                                "groupdesc" => {
                                    layout.group_desc = offset_block;
                                    offset_block += v;
                                }
                                "data map" => {
                                    layout.data_map = offset_block;
                                    offset_block += 1;
                                }
                                "inode map" => {
                                    layout.inode_map = offset_block;
                                    offset_block += 1;
                                }
                                "inode table" => {
                                    layout.inode_table = offset_block;
                                    layout.inode_count = v * (layout.block_size / size_of::<Ext2INode>());
                                    offset_block += v;
                                }
                                "data" => {}
                                _ => {
                                    warn!("unused layout option: {} = {}", name, v)
                                }
                            };
                        }
                        layout.block_count = self.disk_size() / layout.block_size;
                        info!("read fs.layout: {:#?}", layout);
                        super_block = Ext2SuperBlock::from(layout.clone());
                        let group = Ext2GroupDesc::from(layout.clone());
                        // apply settings, enable functions
                        self.filesystem_first_block = if layout.boot { 1 } else { 0 };
                        self.super_block.apply_from(&super_block);
                        self.group_desc_table.clear();
                        self.group_desc_table.push(group);
                        self.seek_block(0)?;
                        // clear disk
                        let block_data = self.create_block_vec();
                        // for i in 0..self.disk_size() / self.block_size() {
                        for i in 0..6 {
                            self.write_data_block(i, &block_data)?;
                        }
                        self.seek_block(0)?;
                        if layout.boot { self.seek_block(1)?; }
                        debug!("write super_block");
                        let mut block_data = self.create_block_vec();
                        block_data[..size_of::<Ext2SuperBlock>()].copy_from_slice(unsafe { serialize_row(&super_block) });
                        self.write_block(&block_data)?;

                        debug!("write group_desc");
                        self.seek_block(self.super_block.s_first_data_block as usize + self.filesystem_first_block)?;
                        let mut block_data = self.create_block_vec();
                        block_data[..size_of::<Ext2GroupDesc>()].copy_from_slice(unsafe { serialize_row(&self.group_desc_table[0]) });
                        self.write_block(&block_data)?;

                        let bg_block_bitmap = self.get_group_desc().bg_block_bitmap as usize;
                        debug!("block bitmap at {} block", bg_block_bitmap);
                        self.seek_block(bg_block_bitmap)?;
                        let bitmap_data_block = self.create_block_vec();
                        self.write_block(&bitmap_data_block)?;
                        self.bitmap_data.clear();
                        self.bitmap_data.extend_from_slice(&bitmap_data_block);

                        let bg_inode_bitmap = self.get_group_desc().bg_inode_bitmap as usize;
                        debug!("inode bitmap at {} block", bg_inode_bitmap);
                        self.seek_block(bg_inode_bitmap)?;
                        let bitmap_inode = self.create_block_vec();
                        self.write_block(&bitmap_inode)?;
                        self.bitmap_inode.clear();
                        self.bitmap_inode.extend_from_slice(&bitmap_inode);

                        // create root directory
                        // self.make_node(1, "..", 0o755, Ext2FileType::Directory)?;
                        // self.make_node(1, ".", 0o755, Ext2FileType::Directory)?;
                        debug!("setting inode bit for root directory");
                        RFS::<T>::bitmap_set(&mut self.bitmap_inode, EXT2_ROOT_INO);
                        let inode_block_number = self.get_group_desc().bg_inode_bitmap as usize;
                        let bitmap_data_clone = self.bitmap_inode.clone();
                        self.write_data_block(inode_block_number, &bitmap_data_clone)?;
                        self.make_node(1, ".", 0o755, Ext2FileType::Directory)?;
                        // self.make_node(EXT2_ROOT_INO, "lost+found", 0o755, Ext2FileType::Directory)?;
                    }
                }
            }
        } else {
            info!("FileSystem found!");
            debug!("fs: {:x?}", super_block);
        }
        self.super_block.apply_from(&super_block);
        // read block group desc table
        debug!("first start block: {}", self.super_block.s_first_data_block);
        self.seek_block(self.super_block.s_first_data_block as usize + self.filesystem_first_block)?;
        let mut data_block = self.create_block_vec();
        self.read_block(&mut data_block)?;
        // just assert there is only one group now
        let group: Ext2GroupDesc = unsafe { deserialize_row(&data_block) };
        // debug!("group desc data: {:x?}", data_block);
        debug!("group: {:x?}", group);
        self.group_desc_table.push(group);

        let bg_block_bitmap = self.get_group_desc().bg_block_bitmap as usize;
        debug!("block bitmap at {} block", bg_block_bitmap);
        self.seek_block(bg_block_bitmap)?;
        let mut bitmap_data_block = self.create_block_vec();
        // ino 1 and 2 reserved
        bitmap_data_block[0] = 0x3;
        self.read_block(&mut bitmap_data_block)?;
        debug!("block bit map: {:?}", &bitmap_data_block[..32]);
        self.bitmap_data.clear();
        self.bitmap_data.extend_from_slice(&bitmap_data_block);

        let bg_inode_bitmap = self.get_group_desc().bg_inode_bitmap as usize;
        debug!("inode bitmap at {} block", bg_inode_bitmap);
        self.seek_block(bg_inode_bitmap)?;
        let mut bitmap_inode = self.create_block_vec();
        self.read_block(&mut bitmap_inode)?;
        debug!("inode bit map: {:?}", &bitmap_inode[..32]);
        self.bitmap_inode.clear();
        self.bitmap_inode.extend_from_slice(&bitmap_inode);

        // load root dir
        self.root_dir = self.get_inode(EXT2_ROOT_INO)?;
        debug!("root dir inode: {:?}", self.root_dir);

        self.print_stats();
        debug!("Init done.");
        Ok(())
    }

    pub fn rfs_destroy(&mut self) -> Result<()> {
        self.get_driver().ddriver_close()
    }

    pub fn rfs_lookup(&mut self, parent: usize, name: &str) -> Result<(usize, Ext2INode)> {
        let parent = RFS::<T>::shift_ino(parent);
        let entries = self.get_dir_entries(parent)?;
        for d in entries {
            debug!("dir entry [{}] {} type {}", d.inode, d.get_name(), d.file_type);
            if d.get_name() == name {
                return Ok((d.inode as usize, self.get_inode(d.inode as usize)?));
            }
        }
        Err(anyhow!("file not found"))
    }

    pub fn rfs_setattr(&mut self, ino: u64, mode: Option<u32>,
                       uid: Option<u32>, gid: Option<u32>, size: Option<u64>,
                       atime: Option<SystemTime>, mtime: Option<SystemTime>,
                       chgtime: Option<SystemTime>,
                       bkuptime: Option<SystemTime>, flags: Option<u32>) -> Result<Ext2INode> {
        let ino = RFS::<T>::shift_ino(ino as usize);
        let mut node = self.get_inode(ino)?;
        match mode {
            Some(v) => node.i_mode = v as u16,
            _ => {}
        };
        match uid {
            Some(v) => {
                node.i_uid = (v & 0xFF) as u16;
                node.i_uid_high = (v >> 16) as u16;
            }
            _ => {}
        };
        match gid {
            Some(v) => {
                node.i_gid = (v & 0xFF) as u16;
                node.i_gid_high = (v >> 16) as u16;
            }
            _ => {}
        };
        match size {
            Some(v) => {
                node.i_size = (v & 0xFFFF) as u32;
                node.i_size_high = (v >> 32) as u32;
            }
            _ => {}
        };
        match atime {
            Some(v) => node.i_atime = v.duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            _ => {}
        };
        match mtime {
            Some(v) => node.i_mtime = v.duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            _ => {}
        };
        match chgtime {
            Some(v) => node.i_ctime = v.duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            _ => {}
        };
        match bkuptime {
            // not checked
            Some(v) => node.i_dtime = v.duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            _ => {}
        };
        match flags {
            Some(v) => node.i_flags = v,
            _ => {}
        };
        self.set_inode(ino, &node)?;
        Ok(node)
    }

    pub fn rfs_read(&mut self, ino: u64, offset: i64, size: u32) -> Result<Vec<u8>> {
        debug!("#read: offset = {:x}, size = {:x}", offset, size);
        let mut offset = offset as usize;
        let size = size as usize;
        let sz = self.block_size();
        let ino = RFS::<T>::shift_ino(ino as usize);
        let mut blocks: Vec<usize> = vec![];
        let start_index = offset / self.block_size();
        assert_eq!(offset % self.block_size(), 0);

        {
            let inode = self.get_inode(ino)?;
            debug!("read inode blocks: {:?} ++ {} ++ {} ++ {}",
            &inode.i_block[..12], inode.i_block[12], inode.i_block[13], inode.i_block[14]);
        }

        let disk_size = self.disk_size();
        let mut last_index = 0 as usize;
        let mut last_block = 0 as usize;
        // rep!(reply, self.walk_blocks_inode(ino, start_index, &mut |block, index| {
        self.visit_blocks_inode(ino, start_index, &mut |block, index| {
            let will_continue = (index + 1) * sz - offset < size;
            debug!("read walk to block {} index {}, continue={}, offset now={}, size now = {}=={}",
                block, index, will_continue, (index+1) * sz, (index+1) * sz - offset, blocks.len() * sz);
            if block == 0 {
                debug!("zero block!");
                return Ok((will_continue, false));
            }
            blocks.push(block);
            if block * sz > disk_size {
                panic!("error block number {:x}!", block);
            }
            // Ok((index + 1 - start_index) * sz < size)
            if last_index != 0 && last_index + 1 != index {
                panic!("error index increase! index now: {}", index);
            }
            last_index = index;
            if last_block != 0 && last_block > block {
                error!("error block increase! block now: {}, last block: {}", block, last_block);
            }
            last_block = block;
            Ok((will_continue, false))
        })?;
        debug!("reading blocks: {:?}", blocks);
        let mut data: Vec<u8> = [0 as u8].repeat(size);
        for (i, block) in blocks.iter().enumerate() {
            // if i * sz >= size { break; }
            let right = min((i + 1) * sz, size);
            self.read_data_block(*block, &mut data[(i * sz)..right])?;
            offset += right - (i * sz);
        }
        Ok(data)
    }

    pub fn rfs_write(&mut self, ino: u64, offset: i64, data: &[u8]) -> Result<u32> {
        let size = data.len() as usize;
        debug!("#write: offset = {:x}, size = {:x}", offset, size);
        let mut offset = offset as usize;
        let base = offset;
        let sz = self.block_size();
        let ino = RFS::<T>::shift_ino(ino as usize);
        let start_index = offset as usize / self.block_size();
        assert_eq!(offset % self.block_size(), 0);

        let mut blocks: Vec<usize> = vec![];

        let disk_size = self.disk_size();
        let mut last_index = 0 as usize;
        let mut last_block = 0 as usize;
        let mut last_zero_index = usize::MAX;
        assert_eq!(0, offset % sz);
        // rep!(reply, self.walk_blocks_inode(ino, start_index, &mut |block, index| {
        self.visit_blocks_inode(ino, start_index, &mut |block, index| {
            let will_continue = (index + 1) * sz - offset < size;
            debug!("write walk to block {} index {}, continue={}, offset now={}, size now = {}, size total = {}",
                block, index, will_continue, (index+1) * sz, (index+1) * sz - offset, size);
            if block == 0 {
                debug!("zero block!");
                if last_zero_index == index {
                    panic!("error zero index");
                }
                last_zero_index = index;
                return Ok((will_continue, index * sz - offset < size));
            }
            blocks.push(block);
            if block * sz > disk_size {
                panic!("error block number {:x}!", block);
            }
            // Ok((index + 1 - start_index) * sz < size)
            if last_index != 0 && last_index + 1 != index {
                panic!("error index increase! index now: {}", index);
            }
            last_index = index;
            if last_block != 0 && last_block > block {
                error!("error block increase! block now: {}, last block: {}", block, last_block);
            }
            last_block = block;
            Ok((will_continue, false))
        })?;
        debug!("writing blocks: {:?}", blocks);
        for (i, block) in blocks.iter().enumerate() {
            // if i * sz >= size { break; }
            let right = min((i + 1) * sz, size);
            self.write_data_block(*block, &data[(i * sz)..right])?;
            offset += right - (i * sz);
        }
        debug!("update file stats");
        let mut inode = self.get_inode(ino)?;
        let filesize = inode.i_size as i64 | ((inode.i_size_high as i64) << 32);
        if offset as i64 > filesize {
            // TODO: large file
            inode.i_size = offset as u32;
            inode.i_size_high = (offset >> 32) as u32;
            self.set_inode(ino, &inode)?;
        }
        let written = offset - base;
        debug!("#write: reply written = {}", written);
        Ok(written as u32)
    }

    pub fn rfs_readdir(&mut self, ino: u64, offset: i64) -> Result<Vec<Ext2DirEntry>> {
        let ino = RFS::<T>::shift_ino(ino as usize);
        let entries = self.get_dir_entries(ino)?.into_iter()
            .skip(offset as usize).collect::<Vec<Ext2DirEntry>>();
        Ok(entries)
    }
}
