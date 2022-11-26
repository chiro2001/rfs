use anyhow::Result;
use std::mem::size_of;

#[derive(Default, Debug)]
pub struct DiskStats {
    pub write_cnt: u32,
    pub read_cnt: u32,
    pub seek_cnt: u32,
}

#[derive(Debug)]
pub struct DiskConst {
    pub read_lat: u32,
    pub write_lat: u32,
    pub seek_lat: u32,
    pub track_num: i32,
    pub major_num: i32,
    pub layout_size: u32,
    pub iounit_size: u32,
}

#[derive(Default, Debug)]
pub struct DiskInfo {
    pub stats: DiskStats,
    pub consts: DiskConst,
}

impl DiskConst {
    pub fn disk_block_count(self: &Self) -> usize {
        (self.layout_size / self.iounit_size).try_into().unwrap()
    }
}

impl Default for DiskConst {
    fn default() -> Self {
        Self {
            read_lat: 2,
            write_lat: 1,
            seek_lat: 4,
            track_num: 0,
            major_num: 100,
            layout_size: 4 * 0x400 * 0x400,
            iounit_size: 512,
        }
    }
}

pub enum SeekType {
    Set,
    Cur,
    End,
}

impl SeekType {
    pub fn to_int(self: &Self) -> i32 {
        match self {
            SeekType::Set => 0,
            SeekType::Cur => 1,
            SeekType::End => 2,
        }
    }
}

/// DiskDriver abstract interface
pub trait DiskDriver {
    fn ddriver_open(self: &mut Self, path: &str) -> Result<()>;
    fn ddriver_close(self: &mut Self) -> Result<()>;
    fn ddriver_seek(self: &mut Self, offset: i64, whence: SeekType) -> Result<u64>;
    fn ddriver_write(self: &mut Self, buf: &[u8], size: usize) -> Result<usize>;
    fn ddriver_read(self: &mut Self, buf: &mut [u8], size: usize) -> Result<usize>;
    fn ddriver_ioctl(self: &mut Self, cmd: u32, arg: &mut [u8]) -> Result<()>;
    fn ddriver_reset(self: &mut Self) -> Result<()>;
}

pub const IOC_REQ_DEVICE_SIZE: u32 = ((2 as u32) << (((0 + 8) + 8) + 14)) | (('A' as u32) << (0 + 8)) | ((0) << 0) | ((size_of::<u32>() as u32) << ((0 + 8) + 8));
pub const IOC_REQ_DEVICE_STATE: u32 = ((2 as u32) << (((0 + 8) + 8) + 14)) | (('A' as u32) << (0 + 8)) | ((1) << 0) | ((size_of::<u32>() as u32 * 3) << ((0 + 8) + 8));
pub const IOC_REQ_DEVICE_RESET: u32 = ((0 as u32) << (((0 + 8) + 8) + 14)) | (('A' as u32) << (0 + 8)) | ((2) << 0) | ((0) << ((0 + 8) + 8));
pub const IOC_REQ_DEVICE_IO_SZ: u32 = ((2 as u32) << (((0 + 8) + 8) + 14)) | (('A' as u32) << (0 + 8)) | ((3) << 0) | ((size_of::<u32>() as u32) << ((0 + 8) + 8));

pub mod memory;
pub mod file;

fn driver_tester(driver: &mut dyn DiskDriver) -> Result<()> {
    driver.ddriver_open("/home/chiro/ddriver")?;
    let mut buf = [0; size_of::<u32>()];
    driver.ddriver_ioctl(IOC_REQ_DEVICE_SIZE, &mut buf)?;
    let disk_size = u32::from_be_bytes(buf.clone()) as usize;
    driver.ddriver_ioctl(IOC_REQ_DEVICE_IO_SZ, &mut buf)?;
    let disk_unit = u32::from_be_bytes(buf) as usize;
    println!("disk size: {}, disk unit: {}", disk_size, disk_unit);
    let write_data = [0x55 as u8].repeat(disk_unit);
    driver.ddriver_write(&write_data, disk_unit)?;
    driver.ddriver_seek(0, SeekType::Set)?;
    let mut read_data = [0 as u8].repeat(disk_unit);
    driver.ddriver_read(&mut read_data, disk_unit)?;
    // println!("write {:?}, read {:?}", write_data, read_data);
    assert_eq!(read_data, write_data);
    driver.ddriver_close()?;
    Ok(())
}