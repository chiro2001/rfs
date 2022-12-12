extern crate core;

mod rfs_lib;

use lazy_static::lazy_static;
use mut_static::MutStatic;
pub use rfs_lib::*;

lazy_static! {
    // Store static mount point argument for signal call use
    pub static ref MOUNT_POINT: MutStatic<String> = MutStatic::new();
    pub static ref DEVICE_FILE: MutStatic<String> = MutStatic::new();
    pub static ref FORCE_FORMAT: MutStatic<bool> = MutStatic::new();
    pub static ref MKFS_FORMAT: MutStatic<bool> = MutStatic::new();
    pub static ref LAYOUT_FILE: MutStatic<String> = MutStatic::new();
    pub static ref ENABLE_CACHING: MutStatic<bool> = MutStatic::new();
}

#[cxx::bridge]
mod ffi {
    extern "Rust" {
        fn add(left: usize, right: usize) -> usize;
    }
}

pub fn add(left: usize, right: usize) -> usize {
    left + right
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
