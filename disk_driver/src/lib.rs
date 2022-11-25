use anyhow::Result;

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
    pub layout_size: u64,
    pub iounit_size: u32,
}

#[derive(Default, Debug)]
pub struct DiskInfo {
    pub stats: DiskStats,
    pub consts: DiskConst,
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

pub mod memory;
