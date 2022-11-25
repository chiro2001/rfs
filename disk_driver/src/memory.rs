use crate::{DiskConst, DiskDriver, DiskInfo, SeekType};
use anyhow::Result;
use crate::*;
use std::mem::size_of;

const MEM_DISK_SIZE: usize = 4 * 0x400 * 0x400;
const IOC_REQ_DEVICE_SIZE: u32 = (((2 as u32) << (((0 + 8) + 8) + 14)) | (('A' as u32) << (0 + 8)) | ((0) << 0) | ((size_of::<u32>() as u32) << ((0 + 8) + 8)));
const IOC_REQ_DEVICE_STATE: u32 = (((2 as u32) << (((0 + 8) + 8) + 14)) | (('A' as u32) << (0 + 8)) | ((1) << 0) | ((size_of::<u32>() as u32 * 3) << ((0 + 8) + 8)));
const IOC_REQ_DEVICE_RESET: u32 = (((0 as u32) << (((0 + 8) + 8) + 14)) | (('A' as u32) << (0 + 8)) | ((2) << 0) | ((0) << ((0 + 8) + 8)));
const IOC_REQ_DEVICE_IO_SZ: u32 = (((2 as u32) << (((0 + 8) + 8) + 14)) | (('A' as u32) << (0 + 8)) | ((3) << 0) | ((size_of::<u32>() as u32) << ((0 + 8) + 8)));

pub struct MemoryDiskDriver {
    pub info: DiskInfo,
    pub mem: [u8; MEM_DISK_SIZE],
    pointer: usize,
}

impl DiskDriver for MemoryDiskDriver {
    fn ddriver_open(self: &mut Self, path: &str) -> Result<()> {
        Ok(())
    }

    fn ddriver_close(self: &mut Self) -> Result<()> {
        Ok(())
    }

    fn ddriver_seek(self: &mut Self, offset: i64, whence: SeekType) -> Result<u64> {
        match whence {
            SeekType::Set => self.pointer = offset as usize,
            SeekType::Cur => self.pointer = (self.pointer as i64 + offset) as usize,
            SeekType::End => self.pointer = (self.info.consts.layout_size - offset as u64) as usize,
        };
        Ok(self.pointer as u64)
    }

    fn ddriver_write(self: &mut Self, buf: &[u8], size: usize) -> Result<usize> {
        assert!(buf.len() >= size);
        self.get_pointer_slice().copy_from_slice(&buf[..size]);
        self.pointer += size;
        Ok((size))
    }

    fn ddriver_read(self: &mut Self, buf: &mut [u8], size: usize) -> Result<usize> {
        buf[..size].copy_from_slice(self.get_pointer_slice());
        self.pointer += size;
        Ok((size))
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
            IOC_REQ_DEVICE_IO_SZ => todo!(),
            _ => Ok(())
        }
    }

    fn ddriver_reset(self: &mut Self) -> Result<()> {
        self.mem.copy_from_slice(&[0; MEM_DISK_SIZE]);
        // TODO: write superblock to erase all filesystem
        self.info = DiskInfo::default();
        self.pointer = 0;
        Ok(())
    }
}

impl MemoryDiskDriver {
    pub fn new() -> Self {
        Self {
            info: DiskInfo {
                stats: Default::default(),
                consts: DiskConst {
                    layout_size: MEM_DISK_SIZE as u64,
                    ..Default::default()
                },
            },
            mem: [0 as u8; MEM_DISK_SIZE],
            pointer: 0,
        }
    }

    fn get_pointer_slice(self: &mut Self) -> &mut [u8] {
        &mut self.mem[self.pointer..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    fn driver_tester(driver: &mut dyn DiskDriver) -> Result<()> {
        driver.ddriver_open("test");
        Ok(())
    }

    #[test]
    fn simple_test() -> Result<()> {
        let mut driver = MemoryDiskDriver::new();
        driver_tester(&mut driver)
    }
}
