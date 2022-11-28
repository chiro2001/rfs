use core::panicking::panic;
/// Filesystem logics
use std::time::Duration;
pub use disk_driver;
use anyhow::{anyhow, Result};
use disk_driver::{DiskDriver, DiskInfo, SeekType};
use log::*;

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
    // ext2 may has boot reserved 1 block prefix
    pub filesystem_first_block: usize,
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
    pub fn create_block_vec(self: &Self) -> Vec<u8> {
        [0 as u8].repeat(self.block_size())
    }

    /// Create a Vec<u8> in multi-blocks size
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
        prv!(ino, block_number, offset / EXT2_INODE_SIZE);
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
        buf[offset..].copy_from_slice(unsafe { serialize_row(inode) });
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

    /// Read all directory entries in one block
    pub fn get_block_dir_entries(self: &mut Self, block: usize) -> Result<Vec<Ext2DirEntry>> {
        let data_block = self.get_data_block(block)?;
        let mut p = 0;
        let mut dirs = vec![];
        while p <= data_block.len() {
            let dir: Ext2DirEntry = unsafe { deserialize_row(&data_block[p..]) };
            if dir.inode == 0 || dir.inode >= self.super_block.s_inodes_count || dir.rec_len == 0 {
                break;
            }
            // info!("[p {:x}] name_len = {}, rec_len = {}", p, dir.name_len, dir.rec_len);
            p += dir.rec_len as usize;
            // info!("next p: {:x}; dir: {}", p, dir.to_string());
            dirs.push(dir);
        }
        // info!("last dir entry: {:?}", dirs.last().unwrap());
        Ok(dirs)
    }

    /// Read all directory entries by ino
    pub fn get_dir_entries(self: &mut Self, ino: usize) -> Result<Vec<Ext2DirEntry>> {
        let inode = self.get_inode(ino)?;
        prv!(inode);
        // TODO: walk all blocks, including indirect blocks
        self.get_block_dir_entries(inode.i_block[0] as usize)
    }

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

    pub fn walk_blocks<F>(self: &mut Self, layer: usize, start_block: usize, block_index: usize, f: &F) -> Result<bool>
        where F: Fn(usize) -> Result<bool> {
        if start_block == 0 { return Ok(false); }
        if layer == 0 {
            return Ok(f(start_block)?);
        }
        let layer_size = self.block_size() / 4;
        let layer_size_mask = layer_size - 1;
        let mut data_block = self.create_block_vec();
        let mut buf_u32 = [0 as u8; 4];
        self.read_data_block(start_block, &mut data_block)?;
        for i in block_index..self.threshold(layer) {
            let o = ((i - self.threshold(layer - 1)) >> (2 * (layer - 1))) & layer_size_mask;
            buf_u32.copy_from_slice(&data_block[o..o + 4]);
            let block = u32::from_be_bytes(buf_u32.clone()) as usize;
            self.walk_blocks(layer - 1, block, i, f)?;
        }
        Ok(true)
    }

    pub fn walk_blocks_inode<F>(self: &mut Self, ino: usize, block_index: usize, f: F) -> Result<()>
        where F: Fn(usize) -> Result<bool> {
        let inode = self.get_inode(ino)?;
        if block_index < self.threshold(0) {
            for i in block_index..self.threshold(0) {
                if inode.i_block[i] == 0 || !f(inode.i_block[i] as usize)? { return Ok(()); }
            }
        } else if block_index < self.threshold(1) {
            self.walk_blocks(1, inode.i_block[12] as usize, block_index, &f)?;
        } else if block_index < self.threshold(2) {
            self.walk_blocks(2, inode.i_block[13] as usize, block_index, &f)?;
        } else if block_index < self.threshold(3) {
            self.walk_blocks(3, inode.i_block[14] as usize, block_index, &f)?;
        } else {
            return Err(anyhow!("Too big block_index!"));
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
