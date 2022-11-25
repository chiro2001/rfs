use crate::{DiskConst, DiskDriver, DiskInfo, SeekType};
use anyhow::Result;

const MEM_DISK_SIZE: usize = 4 * 0x400 * 0x400;

pub struct MemoryDiskDriver<'a> {
    pub info: DiskInfo,
    pub mem: &'a [u8],
    pointer: usize,
}

impl DiskDriver for MemoryDiskDriver<'_> {
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
        Ok((size))
    }

    fn ddriver_read(self: &mut Self, buf: &[u8], size: usize) -> Result<usize> {
        Ok((size))
    }

    fn ddriver_ioctl(self: &mut Self, cmd: u32, arg: &[u8]) -> Result<()> {
        Ok(())
    }
}

impl MemoryDiskDriver<'_> {
    pub fn new() -> Self {
        Self {
            info: DiskInfo {
                stats: Default::default(),
                consts: DiskConst {
                    layout_size: MEM_DISK_SIZE as u64,
                    ..Default::default()
                },
            },
            mem: &[0 as u8; MEM_DISK_SIZE],
            pointer: 0,
        }
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
