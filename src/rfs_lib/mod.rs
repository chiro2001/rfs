use std::time::Duration;
pub use disk_driver;
use anyhow::{Result};
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

const TTL: Duration = Duration::from_secs(1);           // 1 second

pub struct RFS {
    pub driver: Box<dyn DiskDriver>,
    pub driver_info: DiskInfo,
    pub super_block: Ext2SuperBlockMem,
    pub group_desc_table: Vec<Ext2GroupDesc>,
    // ext2 may has boot reserved 1 block prefix
    pub filesystem_first_block: usize,
}

impl RFS {
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

    fn disk_block_size(self: &Self) -> usize { self.driver_info.consts.iounit_size as usize }

    fn disk_size(self: &Self) -> usize { self.driver_info.consts.layout_size as usize }

    fn block_size(self: &Self) -> usize { (1 << self.super_block.s_log_block_size) * 0x400 as usize }

    fn read_disk_block(self: &mut Self, buf: &mut [u8]) -> Result<()> {
        assert_eq!(buf.len(), self.disk_block_size());
        let sz = self.disk_block_size();
        self.driver.ddriver_read(buf, sz)?;
        Ok(())
    }

    fn read_disk_blocks(self: &mut Self, buf: &mut [u8], count: usize) -> Result<()> {
        let sz = self.disk_block_size();
        for i in 0..count { self.read_disk_block(&mut buf[(i * sz)..((i + 1) * sz)])? }
        Ok(())
    }

    fn seek_disk_block(self: &mut Self, index: usize) -> Result<()> {
        let sz = self.disk_block_size();
        // info!("DISK seek to {:x}", index * sz);
        let _n = self.driver.ddriver_seek((index * sz) as i64, SeekType::Set)?;
        Ok(())
    }

    fn block_disk_ratio(self: &Self) -> usize { self.block_size() / self.disk_block_size() }

    pub fn seek_block(self: &mut Self, index: usize) -> Result<()> {
        self.seek_disk_block(index * self.block_disk_ratio())
    }

    pub fn read_block(self: &mut Self, buf: &mut [u8]) -> Result<()> {
        self.read_disk_blocks(buf, self.block_disk_ratio())
    }

    #[allow(dead_code)]
    pub fn read_blocks(self: &mut Self, buf: &mut [u8], count: usize) -> Result<()> {
        self.read_disk_blocks(buf, self.block_disk_ratio() * count)
    }

    pub fn create_block_vec(self: &Self) -> Vec<u8> {
        [0 as u8].repeat(self.block_size())
    }

    pub fn create_blocks_vec(self: &Self, count: usize) -> Vec<u8> {
        [0 as u8].repeat(self.block_size() * count)
    }

    fn get_group_desc(self: &mut Self) -> &Ext2GroupDesc {
        self.group_desc_table.get(0).unwrap()
    }

    pub fn print_stats(self: &Self) {
        info!("fs stats: {}", self.super_block.to_string());
    }

    pub fn get_inode(self: &mut Self, ino: usize) -> Result<Ext2INode> {
        // should ino minus 1?
        let inodes_per_block = self.block_size() / EXT2_INODE_SIZE;
        // assert only one group
        // let block_group = (ino - 1) / inodes_per_block;
        let ino = if ino <= 1 { ino } else { ino - 1 };
        let offset = (ino % inodes_per_block) * EXT2_INODE_SIZE;
        let block_number = ino / inodes_per_block + self.get_group_desc().bg_inode_table as usize;
        prv!(ino, block_number, offset / EXT2_INODE_SIZE);
        let mut buf = self.create_block_vec();
        self.seek_block(block_number)?;
        self.read_block(&mut buf)?;
        Ok(unsafe { deserialize_row(&buf[offset..]) })
    }

    pub fn get_data_block(self: &mut Self, block: usize) -> Result<Vec<u8>> {
        self.seek_block(block)?;
        let mut buf = self.create_block_vec();
        self.read_block(&mut buf)?;
        Ok(buf)
    }

    pub fn read_data_block(self: &mut Self, block: usize, buf: &mut [u8]) -> Result<()> {
        self.seek_block(block)?;
        self.read_block(buf)?;
        Ok(())
    }

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

    pub fn get_dir_entries(self: &mut Self, ino: usize) -> Result<Vec<Ext2DirEntry>> {
        let inode = self.get_inode(ino)?;
        prv!(inode);
        // TODO: walk all blocks, including indirect blocks
        self.get_block_dir_entries(inode.i_block[0] as usize)
    }

    pub fn shift_ino(ino: u64) -> usize {
        // if ino == 1 { EXT2_ROOT_INO } else { ino as usize }
        // if ino == 1 { 0 } else { ino as usize }
        // (ino + 1) as usize
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
