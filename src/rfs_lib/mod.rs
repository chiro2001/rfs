use std::ffi::OsStr;
use std::mem::size_of;
use fuse::{Filesystem, ReplyEntry, Request};
pub use disk_driver;
use disk_driver::{DiskDriver, DiskInfo, IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE};
use libc::c_int;
use anyhow::Result;

pub mod utils;
pub mod desc;
pub mod types;

use desc::Ext2SuperBlock;
use utils::deserialize_row;

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
}

impl RFS {
    pub fn new(driver: Box<dyn DiskDriver>) -> Self {
        Self { driver, driver_info: Default::default() }
    }

    fn disk_block_size(self: &mut Self) -> usize { self.driver_info.consts.iounit_size as usize }

    fn read_block(self: &mut Self, buf: &mut [u8]) -> Result<()> {
        assert_eq!(buf.len(), self.disk_block_size());
        Ok(())
    }

    fn read_blocks(self: &mut Self, buf: &mut [u8], count: usize) -> Result<()> {
        let sz = self.disk_block_size();
        for i in 0..count { self.read_block(&mut buf[(i * sz)..((i + 1) * sz)])? }
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
        let mut data_blocks_head = [0 as u8].repeat((self.disk_block_size() * super_blk_count) as usize);
        result_to_int(self.read_blocks(&mut data_blocks_head, super_blk_count))?;
        let mut super_block: Ext2SuperBlock = unsafe { deserialize_row(&data_blocks_head) };
        println!("read magic: {}", super_block.s_magic);
        if !super_block.magic_matched() {
            println!("fs not found! creating super block...");
            super_block = Ext2SuperBlock::default();
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
