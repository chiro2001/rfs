use std::ffi::OsStr;
use std::mem::size_of;
use fuse::{Filesystem, ReplyEntry, Request};
pub use disk_driver;
use disk_driver::{DiskDriver, DiskInfo, IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE};
use libc::c_int;

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
    pub driver_info: DiskInfo,
}

impl RFS {
    pub fn new(driver: Box<dyn DiskDriver>) -> Self {
        Self { driver, driver_info: Default::default() }
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
        println!("Disk {} has {} blocks.", file, self.driver_info.consts.disk_blocks());
        if self.driver_info.consts.disk_blocks() < 32 {
            println!("Too small disk!");
            return Err(1);
        }
        println!("disk info: {:?}", self.driver_info);
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
