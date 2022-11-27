use std::mem::size_of;
use fuse::{Filesystem, Request};
pub use disk_driver;
use disk_driver::{DiskDriver, DiskInfo, IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE};
use libc::c_int;
use anyhow::Result;

pub mod utils;
pub mod desc;
pub mod types;
pub mod mem;

use desc::Ext2SuperBlock;
use utils::deserialize_row;
use desc::Ext2GroupDesc;
use mem::Ext2SuperBlockMem;

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
}

impl RFS {
    #[allow(dead_code)]
    pub fn new(driver: Box<dyn DiskDriver>) -> Self {
        Self { driver, driver_info: Default::default(), super_block: Default::default() }
    }

    fn disk_block_size(self: &mut Self) -> usize { self.driver_info.consts.iounit_size as usize }

    fn block_size(self: &mut Self) -> usize { (1 << self.super_block.s_log_block_size) as usize }

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
}

fn result_to_int<E: std::fmt::Debug>(res: Result<(), E>) -> Result<(), c_int> {
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
        result_to_int(self.driver.ddriver_open(file))?;
        // get and check size
        let mut buf = [0 as u8; 4];
        result_to_int(self.driver.ddriver_ioctl(IOC_REQ_DEVICE_SIZE, &mut buf))?;
        self.driver_info.consts.layout_size = u32::from_be_bytes(buf.clone());
        result_to_int(self.driver.ddriver_ioctl(IOC_REQ_DEVICE_IO_SZ, &mut buf))?;
        self.driver_info.consts.iounit_size = u32::from_be_bytes(buf.clone());
        // at lease 32 blocks
        println!("Disk {} has {} IO blocks.", file, self.driver_info.consts.disk_block_count());
        if self.driver_info.consts.layout_size < 32 * 0x400 {
            println!("Too small disk!");
            return Err(1);
        }
        println!("disk info: {:?}", self.driver_info);
        // read super block
        let super_blk_count = size_of::<Ext2SuperBlock>() / self.disk_block_size();
        let disk_block_size = self.disk_block_size();
        println!("super block size {} disk block ({} bytes)", super_blk_count, super_blk_count * self.disk_block_size());
        let mut data_blocks_head = [0 as u8].repeat((disk_block_size * super_blk_count) as usize);
        result_to_int(self.read_disk_blocks(&mut data_blocks_head, super_blk_count))?;
        let mut super_block: Ext2SuperBlock = unsafe { deserialize_row(&data_blocks_head) };
        println!("{:?}", data_blocks_head);
        if !super_block.magic_matched() {
            println!("read again.");
            // maybe there is one block reserved for boot,
            // read one block again
            result_to_int(self.read_disk_blocks(&mut data_blocks_head, super_blk_count))?;
            // data_blocks_head.reverse();
            super_block = unsafe { deserialize_row(&data_blocks_head) };
            println!("re-read magic: {}", super_block.s_magic);
        }
        println!("{:?}", data_blocks_head);
        println!("magic read here: {:02x} {:02x}", data_blocks_head[56], data_blocks_head[57]);
        println!("read magic: {}", super_block.s_magic);
        if !super_block.magic_matched() {
            println!("FileSystem not found! creating super block...");
            // let mut group_desc = Ext2GroupDesc::default();
            super_block = Ext2SuperBlock::default();
            // set block size to 1 KiB
            super_block.s_log_block_size = 10;
            // super block use first block (when block size is 1 KiB), set group 0 start block = 1
            super_block.s_first_data_block = 1;
            super_block.s_first_ino = 0;
            // super_block.s_blocks_per_group
            let block_count = self.driver_info.consts.layout_size as usize / super_block.block_size();
            println!("total {} blocks", block_count);
            self.super_block.apply_from(&super_block);
        }
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
