#![allow(dead_code)]
#![allow(unused_variables)]

extern crate core;

use core::mem::{align_of, forget, size_of};
use core::slice::{from_raw_parts, from_raw_parts_mut};
use std::os::raw::c_int;
use fuser::{ReplyAttr, ReplyData, ReplyDirectory, TimeOrNow};
use log::debug;
use std::env::set_var;
use std::time::SystemTime;

pub trait VecExt {
    /// Casts a `Vec<T>` into a `Vec<U>`.
    ///
    /// # Safety
    ///
    /// The caller must ensure the following safety properties:
    ///
    ///   * The vector `self` contains valid elements of type `U`. In
    ///     particular, note that `drop` will never be called for `T`s in `self`
    ///     and instead will be called for the `U`'s in `self`.
    ///   * The size and alignment of `T` and `U` are identical.
    ///
    /// # Panics
    ///
    /// Panics if the size or alignment of `T` and `U` differ.
    unsafe fn cast<U>(self) -> Vec<U>;
}

pub trait SliceExt {
    /// Casts an `&[T]` into an `&[U]`.
    ///
    /// # Safety
    ///
    /// The caller must ensure the following safety properties:
    ///
    ///   * The slice `self` contains valid elements of type `U`.
    ///   * The size of `T` and `U` are identical.
    ///   * The alignment of `T` is an integer multiple of the alignment of `U`.
    ///
    /// # Panics
    ///
    /// Panics if the size of `T` and `U` differ or if the alignment of `T` is
    /// not an integer multiple of `U`.
    unsafe fn cast<'a, U>(&'a self) -> &'a [U];

    /// Casts an `&mut [T]` into an `&mut [U]`.
    ///
    /// # Safety
    ///
    /// The caller must ensure the following safety properties:
    ///
    ///   * The slice `self` contains valid elements of type `U`.
    ///   * The size of `T` and `U` are identical.
    ///   * The alignment of `T` is an integer multiple of the alignment of `U`.
    ///
    /// # Panics
    ///
    /// Panics if the size of `T` and `U` differ or if the alignment of `T` is
    /// not an integer multiple of `U`.
    unsafe fn cast_mut<'a, U>(&'a mut self) -> &'a mut [U];
    unsafe fn cast_mut_force<'a, U>(&'a self) -> &'a mut [U];
}

fn calc_new_len_cap<T, U>(vec: &Vec<T>) -> (usize, usize) {
    if size_of::<T>() > size_of::<U>() {
        assert!(size_of::<T>() % size_of::<U>() == 0);
        let factor = size_of::<T>() / size_of::<U>();
        (vec.len() * factor, vec.capacity() * factor)
    } else if size_of::<U>() > size_of::<T>() {
        assert!(size_of::<U>() % size_of::<T>() == 0);
        let factor = size_of::<U>() / size_of::<T>();
        (vec.len() / factor, vec.capacity() / factor)
    } else {
        (vec.len(), vec.capacity())
    }
}

impl<T> VecExt for Vec<T> {
    unsafe fn cast<U>(mut self) -> Vec<U> {
        assert!(align_of::<T>() == align_of::<U>());

        let (new_len, new_cap) = calc_new_len_cap::<T, U>(&self);
        let new_ptr = self.as_mut_ptr() as *mut U;
        forget(self);

        Vec::from_raw_parts(new_ptr, new_len, new_cap)
    }
}

fn calc_new_len<T, U>(slice: &[T]) -> usize {
    if size_of::<T>() > size_of::<U>() {
        assert!(size_of::<T>() % size_of::<U>() == 0);
        let factor = size_of::<T>() / size_of::<U>();
        slice.len() * factor
    } else if size_of::<U>() > size_of::<T>() {
        assert!(size_of::<U>() % size_of::<T>() == 0);
        let factor = size_of::<U>() / size_of::<T>();
        slice.len() / factor
    } else {
        slice.len()
    }
}

impl<T> SliceExt for [T] {
    unsafe fn cast<U>(&self) -> &[U] {
        assert_eq!(align_of::<T>() % align_of::<U>(), 0);

        let new_len = calc_new_len::<T, U>(self);
        let new_ptr = self.as_ptr() as *const U;
        from_raw_parts(new_ptr, new_len)
    }

    unsafe fn cast_mut<U>(&mut self) -> &mut [U] {
        assert_eq!(align_of::<T>() % align_of::<U>(), 0);

        let new_len = calc_new_len::<T, U>(self);
        let new_ptr = self.as_mut_ptr() as *mut U;
        from_raw_parts_mut(new_ptr, new_len)
    }

    unsafe fn cast_mut_force<U>(&self) -> &mut [U] {
        assert_eq!(align_of::<T>() % align_of::<U>(), 0);

        let new_len = calc_new_len::<T, U>(self);
        let new_ptr = self.as_ptr() as *mut U;
        from_raw_parts_mut(new_ptr, new_len)
    }
}

/// Unsafe data cast
/// struct => &[u8]
pub unsafe fn serialize_row<T: Sized>(src: &T) -> &[u8] {
    from_raw_parts((src as *const T) as *const u8, size_of::<T>())
}

/// Unsafe data cast
/// &[u8] => struct
pub unsafe fn deserialize_row<T>(src: &[u8]) -> T {
    std::ptr::read(src.as_ptr() as *const _)
}

/// Get filed offset from it's struct
#[macro_export]
macro_rules! get_offset {
    ($type:ty, $field:tt) => ({
        let dummy = ::core::mem::MaybeUninit::<$type>::uninit();
        let dummy_ptr = dummy.as_ptr();
        let member_ptr = unsafe { ::core::ptr::addr_of!((*dummy_ptr).$field) };
        member_ptr as usize - dummy_ptr as usize
    })
}

/// Print variable name and it's value
#[macro_export]
macro_rules! prv {
    ($e:expr) => {
        {
            use log::*;
            debug!("{} = {:?}", stringify!($e), $e);
        }
    };
    ($($e:expr),*) => {
        {
            use log::*;
            $(debug!("{} = {:?}, ", stringify!($e), $e);)*
            // debug!("");
        }
    }
}

/// Get Result<()>'s Ok(data)
/// When errors, call reply.error() and return;
#[macro_export]
macro_rules! rep {
    ($reply:expr, $n:ident, $r:expr) => {
        let $n;
        let _result = $r;
        if _result.is_err() {
            $reply.error(ENOENT);
            return;
        } else {
            $n = _result.unwrap();
        }
    };
    ($reply:expr, $r:expr) => {
        rep!($reply, _r, $r);
    };
}

/// Get Result<()>'s Ok(data) as mutable
/// When errors, call reply.error() and return;
#[macro_export]
macro_rules! rep_mut {
    ($reply:expr, $n:ident, $r:expr) => {
        let mut $n;
        let _result = $r;
        if _result.is_err() {
            $reply.error(ENOENT);
            return;
        } else {
            $n = _result.unwrap();
        }
    };
    ($reply:expr, $r:expr) => {
        rep!($reply, _r, $r);
    };
}

/// Convert Result<T, E> to Result<T, c_int>
pub fn ret<E, T>(res: Result<T, E>) -> Result<T, c_int> where E: std::fmt::Debug {
    match res {
        Ok(ok) => Ok(ok),
        Err(e) => {
            println!("RFS Error: {:#?}", e);
            Err(1)
        }
    }
}

/// Reply* has method fn error(err), but have no trait to manage it.
pub trait ReplyError {
    fn make_error(self: Self, err: c_int);
}

impl ReplyError for ReplyAttr {
    fn make_error(self: Self, err: c_int) { self.error(err) }
}

impl ReplyError for ReplyData {
    fn make_error(self: Self, err: c_int) { self.error(err) }
}

impl ReplyError for ReplyDirectory {
    fn make_error(self: Self, err: c_int) { self.error(err) }
}

pub fn up_align(value: usize, align_log: usize) -> usize {
    ((value >> align_log) + 1) << align_log
}

pub fn down_align(value: usize, align_log: usize) -> usize {
    debug!("down_align(value={:x}, align_log={})", value, align_log);
    (value >> align_log) << align_log
}

pub fn show_hex(data: &[u8], group_size: usize) {
    for (i, b) in data.iter().enumerate() {
        print!("{:02x} ", *b);
        if i % group_size == group_size - 1 || i == data.len() - 1 {
            println!();
        }
    }
}

pub fn show_hex_debug(data: &[u8], group_size: usize) {
    let mut v = vec![];
    for (i, b) in data.iter().enumerate() {
        // debug!("{:02x} ", *b);
        v.push(*b);
        if i % group_size == group_size - 1 || i == data.len() - 1 {
            debug!("{}", v.iter().map(|x| format!("{:2x}", x)).collect::<Vec<_>>().join(" "));
            v.clear();
        }
    }
}

pub fn init_logs() {
    let logging_level = std::env::var("RUST_LOG");
    if logging_level.is_err() { set_var("RUST_LOG", "info"); }
    env_logger::init();
}

pub fn time_or_now_convert(t: Option<TimeOrNow>) -> Option<SystemTime> {
    match t {
        None => None,
        Some(t) => Some(match t {
            TimeOrNow::SpecificTime(t) => t,
            TimeOrNow::Now => SystemTime::now(),
        })
    }
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use crate::rfs_lib::desc::Ext2SuperBlock;
    use crate::rfs_lib::utils::deserialize_row;

    #[derive(Debug)]
    #[repr(C, align(8))]
    struct TestStruct {
        pub a: u32,
        pub b: u8,
    }

    #[test]
    fn test_deserialize_row() -> Result<()> {
        let s: TestStruct = unsafe { deserialize_row(&vec![1, 2, 3, 4, 5]) };
        println!("{:x?}", s);
        Ok(())
    }

    #[test]
    fn test_get_offset() -> Result<()> {
        let la = get_offset!(TestStruct, a);
        let lb = get_offset!(TestStruct, b);
        println!("la = {}, lb = {}", la, lb);
        let l_s_inodes_count = get_offset!(Ext2SuperBlock, s_inodes_count);
        println!("l_s_inodes_count = {}", l_s_inodes_count);
        Ok(())
    }
}