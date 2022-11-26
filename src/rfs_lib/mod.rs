use std::ffi::OsStr;
use fuse::{Filesystem, ReplyEntry, Request};
pub use disk_driver;
use disk_driver::{DiskDriver, DiskInfo, IOC_REQ_DEVICE_SIZE};
use libc::c_int;
use crate::desc::Ext2INode;

pub mod utils;
pub mod desc;
pub mod types;

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
    pub driver_info: DiskInfo
}

impl RFS {
    pub fn new(driver: Box<dyn DiskDriver>) -> Self {
        Self { driver, driver_info: Default::default() }
    }
}

impl Filesystem for RFS {
    fn init(&mut self, _req: &Request<'_>) -> Result<(), c_int> {
        self.driver.ddriver_open("disk")?;
        // check size
        let mut buf = [0 as u8; 32];
        self.driver.ddriver_ioctl(IOC_REQ_DEVICE_SIZE, &mut buf)?;
        self.driver_info.consts.layout_size = u32::from_be_bytes()
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
