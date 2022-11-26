use crate::{DiskConst, DiskDriver, DiskInfo, SeekType};
use anyhow::Result;
use crate::*;

const FILE_DISK_SIZE: usize = 4 * 0x400 * 0x400;
const FILE_DISK_UNIT: usize = 512;

pub struct FileDiskDriver {
    pub info: DiskInfo,
    pub mem: Vec<u8>,
    pointer: usize,
}

impl DiskDriver for FileDiskDriver {
    fn ddriver_open(self: &mut Self, path: &str) -> Result<()> {
        println!("FileDrv open: {}", path);
        Ok(())
    }

    fn ddriver_close(self: &mut Self) -> Result<()> {
        Ok(())
    }

    fn ddriver_seek(self: &mut Self, offset: i64, whence: SeekType) -> Result<u64> {
        match whence {
            SeekType::Set => self.pointer = offset as usize,
            SeekType::Cur => self.pointer = (self.pointer as i64 + offset) as usize,
            SeekType::End => self.pointer = (self.info.consts.layout_size as i64 - offset) as usize,
        };
        Ok(self.pointer as u64)
    }

    fn ddriver_write(self: &mut Self, buf: &[u8], size: usize) -> Result<usize> {
        assert!(buf.len() >= size);
        self.get_pointer_slice(size).copy_from_slice(&buf[..size]);
        self.pointer += size;
        Ok(size)
    }

    fn ddriver_read(self: &mut Self, buf: &mut [u8], size: usize) -> Result<usize> {
        buf[..size].copy_from_slice(self.get_pointer_slice(size));
        self.pointer += size;
        Ok(size)
    }

    fn ddriver_ioctl(self: &mut Self, cmd: u32, arg: &mut [u8]) -> Result<()> {
        match cmd {
            IOC_REQ_DEVICE_SIZE => {
                arg[0..4].copy_from_slice(&self.info.consts.layout_size.to_le_bytes());
                Ok(())
            }
            IOC_REQ_DEVICE_STATE => {
                assert_eq!(3 * 4, size_of::<DiskStats>());
                arg[0..4].copy_from_slice(&self.info.stats.write_cnt.to_le_bytes());
                arg[4..8].copy_from_slice(&self.info.stats.read_cnt.to_le_bytes());
                arg[8..12].copy_from_slice(&self.info.stats.seek_cnt.to_le_bytes());
                Ok(())
            }
            IOC_REQ_DEVICE_RESET => {
                self.ddriver_reset()
            }
            IOC_REQ_DEVICE_IO_SZ => {
                arg[0..4].copy_from_slice(&self.info.consts.iounit_size.to_le_bytes());
                Ok(())
            }
            _ => Ok(())
        }
    }

    fn ddriver_reset(self: &mut Self) -> Result<()> {
        self.mem.copy_from_slice(&[0; FILE_DISK_SIZE]);
        // TODO: write superblock to erase all filesystem
        self.info = DiskInfo::default();
        self.pointer = 0;
        Ok(())
    }
}

impl FileDiskDriver {
    pub fn new() -> Self {
        Self {
            info: DiskInfo {
                stats: Default::default(),
                consts: DiskConst {
                    layout_size: FILE_DISK_SIZE as u32,
                    iounit_size: FILE_DISK_UNIT as u32,
                    ..Default::default()
                },
            },
            mem: vec![0 as u8; FILE_DISK_SIZE],
            pointer: 0,
        }
    }

    fn get_pointer_slice(self: &mut Self, size: usize) -> &mut [u8] {
        &mut self.mem[self.pointer..(size + self.pointer)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn simple_test() -> Result<()> {
        let mut driver = FileDiskDriver::new();
        driver_tester(&mut driver)
    }
}
