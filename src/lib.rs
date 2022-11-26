use fuse::Filesystem;
pub use disk_driver;
use disk_driver::DiskDriver;

pub mod utils;

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
}

impl RFS {
    pub fn new(driver: Box<dyn DiskDriver>) -> Self {
        Self { driver }
    }
}

impl Filesystem for RFS {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
