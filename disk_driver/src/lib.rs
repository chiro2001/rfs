#[derive(Default, Debug)]
pub struct DiskStats {
    pub read_cnt: u32,
    pub write_cnt: u32,
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

/// DiskDriver abstract interface
pub trait DiskDriver {
    fn ddriver_open(path: &str) -> i32;
    fn ddriver_close(fd: i32) -> i32;
    fn ddriver_seek(fd: i32, offset: i64, whence: i32) -> i32;
    fn ddriver_write(fd: i32, buf: &[u8], size: usize) -> i32;
    fn ddriver_read(fd: i32, buf: &[u8], size: usize) -> i32;
    fn ddriver_ioctl(fd: i32, cmd: u32, arg: &[u8]) -> i32;
}

pub mod memory;
