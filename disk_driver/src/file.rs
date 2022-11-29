use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use crate::{DiskConst, DiskDriver, DiskInfo, SeekType};
use anyhow::Result;
use log::*;
use crate::*;

/// 4MiB size
const FILE_DISK_SIZE: usize = 4 * 0x400 * 0x400;
/// 1 GiB size
// const FILE_DISK_SIZE: usize = 4 * 0x400 * 0x400 * 0x100;

const FILE_DISK_UNIT: usize = 512;

pub struct FileDiskDriver {
    pub info: DiskInfo,
    pub file: Option<File>,
}

impl FileDiskDriver {
    fn get_file(self: &mut Self) -> &File {
        self.file.as_ref().unwrap()
    }
    fn blank_data(self: &mut Self) -> Vec<u8> {
        [0 as u8].repeat(self.info.consts.layout_size as usize)
    }
}

impl DiskDriver for FileDiskDriver {
    fn ddriver_open(self: &mut Self, path: &str) -> Result<()> {
        info!("FileDrv open: {}", path);
        if !Path::new(path).exists() {
            info!("Create a new file {}", path);
            File::create(path)?.write_all(&self.blank_data())?;
        }
        self.file = Some(OpenOptions::new().read(true).write(true).open(path)?);
        let filesize = self.get_file().metadata()?.len();
        debug!("file size: {}", filesize);
        // padding zero to filesize
        if filesize < self.info.consts.layout_size.into() {
            debug!("too small file, write zeros for padding");
            let padding = self.info.consts.layout_size as usize - filesize as usize;
            self.ddriver_write(&[0 as u8].repeat(padding), padding)?;
            debug!("write done");
            self.file.as_ref().unwrap().flush();
        }
        Ok(())
    }

    fn ddriver_close(self: &mut Self) -> Result<()> {
        self.file.as_ref().unwrap().flush();
        Ok(())
    }

    fn ddriver_seek(self: &mut Self, offset: i64, whence: SeekType) -> Result<u64> {
        if whence == SeekType::Set {
            debug!("disk seek to {:x}", offset);
            if offset > self.info.consts.layout_size.into() {
                panic!("SEEK OUT! size is {:x}, offset = {:x}", self.info.consts.layout_size, offset);
            }
        }
        Ok(self.get_file().seek(match whence {
            SeekType::Set => SeekFrom::Start(offset as u64),
            SeekType::Cur => SeekFrom::Current(offset),
            SeekType::End => SeekFrom::End(offset),
        })?)
    }

    fn ddriver_write(self: &mut Self, buf: &[u8], size: usize) -> Result<usize> {
        assert!(buf.len() >= size);
        let offset = self.file.as_ref().unwrap().stream_position().unwrap() as usize;
        debug!("disk write @ {:x} - {:x}", offset, offset + size);
        self.get_file().write_all(&buf[..size])?;
        Ok(size)
    }

    fn ddriver_read(self: &mut Self, buf: &mut [u8], size: usize) -> Result<usize> {
        Ok(self.get_file().read(&mut buf[..size])?)
    }

    fn ddriver_ioctl(self: &mut Self, cmd: u32, arg: &mut [u8]) -> Result<()> {
        match cmd {
            IOC_REQ_DEVICE_SIZE => {
                arg[0..4].copy_from_slice(&self.info.consts.layout_size.to_be_bytes());
                Ok(())
            }
            IOC_REQ_DEVICE_STATE => {
                assert_eq!(3 * 4, size_of::<DiskStats>());
                arg[0..4].copy_from_slice(&self.info.stats.write_cnt.to_be_bytes());
                arg[4..8].copy_from_slice(&self.info.stats.read_cnt.to_be_bytes());
                arg[8..12].copy_from_slice(&self.info.stats.seek_cnt.to_be_bytes());
                Ok(())
            }
            IOC_REQ_DEVICE_RESET => {
                self.ddriver_reset()
            }
            IOC_REQ_DEVICE_IO_SZ => {
                arg[0..4].copy_from_slice(&self.info.consts.iounit_size.to_be_bytes());
                Ok(())
            }
            _ => Ok(())
        }
    }

    fn ddriver_reset(self: &mut Self) -> Result<()> {
        self.ddriver_write(&[0].repeat(self.info.consts.layout_size as usize), self.info.consts.layout_size.try_into().unwrap())?;
        // TODO: write superblock to erase all filesystem
        self.info = DiskInfo::default();
        Ok(())
    }
}

impl FileDiskDriver {
    pub fn new(path: &str) -> Self {
        Self {
            info: DiskInfo {
                stats: Default::default(),
                consts: DiskConst {
                    layout_size: FILE_DISK_SIZE as u32,
                    iounit_size: FILE_DISK_UNIT as u32,
                    ..Default::default()
                },
            },
            file: if path.is_empty() { None } else { Some(File::open(path).unwrap()) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn simple_test() -> Result<()> {
        let mut driver = FileDiskDriver::new("");
        driver_tester(&mut driver)?;
        info!("Test done.");
        Ok(())
    }
}
