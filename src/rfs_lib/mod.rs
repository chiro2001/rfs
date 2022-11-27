use std::mem::size_of;
use std::process::Stdio;
use fuse::{Filesystem, Request};
pub use disk_driver;
use disk_driver::{DiskDriver, DiskInfo, IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE, SeekType};
use libc::c_int;
use anyhow::Result;
use chrono::Local;
use execute::Execute;

pub mod utils;
pub mod desc;
pub mod types;
pub mod mem;

use desc::Ext2SuperBlock;
use utils::deserialize_row;
use desc::Ext2GroupDesc;
use mem::Ext2SuperBlockMem;
use crate::get_offset;

#[cxx::bridge]
mod ffi {
    extern "Rust" {
        fn add(left: usize, right: usize) -> usize;
    }
}

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

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
        let _n = self.driver.ddriver_seek((index * sz) as i64, SeekType::Set)?;
        Ok(())
    }

    fn block_disk_ratio(self: &Self) -> usize { self.block_size() / self.disk_block_size() }

    fn seek_block(self: &mut Self, index: usize) -> Result<()> {
        self.seek_disk_block((index + self.filesystem_first_block) * self.block_disk_ratio())
    }

    fn read_block(self: &mut Self, buf: &mut [u8]) -> Result<()> {
        self.read_disk_blocks(buf, self.block_disk_ratio())
    }

    fn read_blocks(self: &mut Self, buf: &mut [u8], count: usize) -> Result<()> {
        self.read_disk_blocks(buf, self.block_disk_ratio() * count)
    }

    fn create_block_vec(self: &Self) -> Vec<u8> {
        [0 as u8].repeat(self.block_size())
    }

    fn get_group_desc(self: &mut Self) -> &Ext2GroupDesc {
        self.group_desc_table.get(0).unwrap()
    }

    fn print_stats(self: &Self) {
        println!("fs stats: {}", self.super_block.to_string());
    }
}

fn rret<E: std::fmt::Debug>(res: Result<(), E>) -> Result<(), c_int> {
    match res {
        Ok(()) => Ok(()),
        Err(e) => {
            println!("RFS Error: {:#?}", e);
            Err(1)
        }
    }
}

impl Filesystem for RFS {
    fn init(&mut self, _req: &Request<'_>) -> Result<(), c_int> {
        let file = "disk";
        rret(self.driver.ddriver_open(file))?;
        // get and check size
        let mut buf = [0 as u8; 4];
        rret(self.driver.ddriver_ioctl(IOC_REQ_DEVICE_SIZE, &mut buf))?;
        self.driver_info.consts.layout_size = u32::from_be_bytes(buf.clone());
        rret(self.driver.ddriver_ioctl(IOC_REQ_DEVICE_IO_SZ, &mut buf))?;
        self.driver_info.consts.iounit_size = u32::from_be_bytes(buf.clone());
        // at lease 32 blocks
        println!("Disk {} has {} IO blocks.", file, self.driver_info.consts.disk_block_count());
        if self.disk_size() < 32 * 0x400 {
            println!("Too small disk!");
            return Err(1);
        }
        println!("disk info: {:?}", self.driver_info);
        // read super block
        let super_blk_count = size_of::<Ext2SuperBlock>() / self.disk_block_size();
        let disk_block_size = self.disk_block_size();
        println!("super block size {} disk block ({} bytes)", super_blk_count, super_blk_count * self.disk_block_size());
        let mut data_blocks_head = [0 as u8].repeat((disk_block_size * super_blk_count) as usize);
        rret(self.read_disk_blocks(&mut data_blocks_head, super_blk_count))?;
        let mut super_block: Ext2SuperBlock = unsafe { deserialize_row(&data_blocks_head) };
        if !super_block.magic_matched() {
            // maybe there is one block reserved for boot,
            // read one block again
            rret(self.read_disk_blocks(&mut data_blocks_head, super_blk_count))?;
            // data_blocks_head.reverse();
            super_block = unsafe { deserialize_row(&data_blocks_head) };
            if super_block.magic_matched() { self.filesystem_first_block = 1; }
        }
        if !super_block.magic_matched() {
            println!("FileSystem not found! creating super block...");
            // let mut group_desc = Ext2GroupDesc::default();
            super_block = Ext2SuperBlock::default();
            // set block size to 1 KiB
            super_block.s_log_block_size = 10;
            // super block use first block (when block size is 1 KiB), set group 0 start block = 1;
            // block size bigger than 2 KiB, use 0
            super_block.s_first_data_block = if self.block_size() < 2 * 0x400 { 1 } else { 0 };
            // super_block.s_first_ino = 0 .. 11;
            // It can be bigger than disk... why? use default values
            // super_block.s_blocks_per_group = 8192;
            // super_block.s_clusters_per_group = 8192;
            // super_block.s_inodes_per_group = 1024;
            // 4 KiB / inode
            super_block.s_inodes_count = (self.disk_size() / 0x400 / 4) as u32;
            let block_count = self.disk_size() / super_block.block_size();
            super_block.s_blocks_count = block_count as u32;
            super_block.s_free_inodes_count = super_block.s_inodes_count;
            super_block.s_free_blocks_count = super_block.s_blocks_count;

            // timestamps
            let dt = Local::now();
            super_block.s_wtime = dt.timestamp_millis() as u32;
            println!("total {} blocks", block_count);
            // TODO: create layout
            // let's use mkfs.ext2
            let mut command = execute::command_args!("mkfs.ext2", file);
            command.stdout(Stdio::piped());
            let output = command.execute_output().unwrap();
            println!("{}", String::from_utf8(output.stdout).unwrap());
            // reload disk driver
            rret(self.driver.ddriver_close())?;
            rret(self.driver.ddriver_open(file))?;
            rret(self.seek_block(0))?;
            rret(self.read_disk_blocks(&mut data_blocks_head, super_blk_count))?;
            super_block = unsafe { deserialize_row(&data_blocks_head) };
            if !super_block.magic_matched() {
                rret(self.read_disk_blocks(&mut data_blocks_head, super_blk_count))?;
                super_block = unsafe { deserialize_row(&data_blocks_head) };
            }
            if super_block.magic_matched() {
                self.filesystem_first_block = 1;
                println!("Disk driver reloaded.");
            } else {
                println!("Make filesystem failed!");
                return Err(1);
            }
        } else {
            println!("FileSystem found!");
            println!("fs: {:?}", super_block);
        }
        self.super_block.apply_from(&super_block);
        // println!("s_log_block_size = {}", super_block.s_log_block_size);
        self.print_stats();
        // read block group desc table
        println!("first start block: {}", self.super_block.s_first_data_block);
        rret(self.seek_block(self.super_block.s_first_data_block as usize))?;
        let mut data_block = self.create_block_vec();
        rret(self.read_block(&mut data_block))?;
        // just assert there is only one group now
        let group: Ext2GroupDesc = unsafe { deserialize_row(&data_block) };
        println!("group desc data: {:x?}", data_block);
        println!("group: {:?}", group);
        self.group_desc_table.push(group);
        let bg_block_bitmap = self.get_group_desc().bg_block_bitmap as usize;
        println!("block bitmap at {} block", bg_block_bitmap);
        rret(self.seek_block(bg_block_bitmap))?;
        let mut bitmap_data_block = self.create_block_vec();
        rret(self.read_block(&mut bitmap_data_block))?;

        let bg_inode_bitmap = self.get_group_desc().bg_inode_bitmap as usize;
        println!("inode bitmap at {} block", bg_inode_bitmap);
        rret(self.seek_block(bg_inode_bitmap))?;
        let mut bitmap_inode = self.create_block_vec();
        rret(self.read_block(&mut bitmap_inode))?;
        println!("inode bit map: {:?}", &bitmap_inode[..32]);

        let bg_inode_table = self.get_group_desc().bg_inode_table as usize;
        println!("inode table start at {} block", bg_inode_table);
        rret(self.seek_block(bg_inode_table))?;
        let mut bg_inode_table = self.create_block_vec();
        rret(self.read_block(&mut bg_inode_table))?;
        println!("inode table: {:?}", &bg_inode_table[..32]);
        println!("Init done.");
        Ok(())
    }

    fn destroy(&mut self, _req: &Request<'_>) {
        self.driver.ddriver_close().unwrap();
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
