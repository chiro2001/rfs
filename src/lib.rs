extern crate core;

mod rfs_lib;

use lazy_static::lazy_static;
use mut_static::MutStatic;
pub use rfs_lib::*;

lazy_static! {
    /// Store static mount point argument for signal call use
    pub static ref MOUNT_POINT: MutStatic<String> = MutStatic::new();
    pub static ref DEVICE_FILE: MutStatic<String> = MutStatic::new();
    pub static ref FORCE_FORMAT: MutStatic<bool> = MutStatic::new();
}