use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use crate::{DiskConst, DiskDriver, DiskInfo, SeekType};
use anyhow::Result;
use log::*;
use crate::*;

/// 4MiB size
const FILE_DISK_SIZE: usize = 4 * 0x400 * 0x400;
/// 1 GiB size
// const FILE_DISK_SIZE: usize = 1 * 0x400 * 0x400 * 0x400;

const FILE_DISK_UNIT: usize = 512;

pub struct FileDiskDriver {
    pub info: DiskInfo,
    pub file: Option<File>,
    pub latency: bool,
}

impl FileDiskDriver {
    fn get_file(&mut self) -> &File {
        self.file.as_ref().unwrap()
    }
    fn blank_data(&mut self) -> Vec<u8> {
        [0 as u8].repeat(self.info.consts.layout_size as usize)
    }
}

impl DiskDriver for FileDiskDriver {
    fn ddriver_open(&mut self, path: &str) -> Result<()> {
        if self.file.is_some() {
            self.ddriver_close()?;
        }
        info!("FileDrv open: {}", path);
        if !Path::new(path).exists() {
            info!("Create a new file {}", path);
            File::create(path)?.write_all(&self.blank_data())?;
        }
        self.file = Some(OpenOptions::new().read(true).write(true).open(path)?);
        let filesize = self.get_file().metadata()?.len();
        debug!("disk size: 0x{:x}; file size: 0x{:x}", self.info.consts.layout_size, filesize);
        // padding zero to filesize
        if filesize < self.info.consts.layout_size.into() {
            debug!("too small file, write zeros for padding");
            let padding = self.info.consts.layout_size as usize - filesize as usize;
            self.ddriver_write(&[0 as u8].repeat(padding), padding)?;
            debug!("write done");
            self.get_file().flush()?;
        }
        Ok(())
    }

    fn ddriver_close(&mut self) -> Result<()> {
        self.get_file().flush()?;
        Ok(())
    }

    fn ddriver_seek(&mut self, offset: i64, whence: SeekType) -> Result<u64> {
        if whence == SeekType::Set {
            debug!("disk seek to {:x}", offset);
            if offset > self.info.consts.layout_size.into() {
                panic!("SEEK OUT! size is 0x{:x}, offset = 0x{:x}", self.info.consts.layout_size, offset);
            }
        }
        if self.latency {
            let delay_seek = Duration::from_millis(self.info.consts.seek_lat as u64);
            // println!("delay_seek: {:?}", delay_seek);
            sleep(delay_seek);
        }
        Ok(self.get_file().seek(match whence {
            SeekType::Set => SeekFrom::Start(offset as u64),
            SeekType::Cur => SeekFrom::Current(offset),
            SeekType::End => SeekFrom::End(offset),
        })?)
    }

    fn ddriver_write(&mut self, buf: &[u8], size: usize) -> Result<usize> {
        assert!(buf.len() >= size);
        let offset = self.file.as_ref().unwrap().stream_position().unwrap() as usize;
        debug!("disk write @ {:x} - {:x}", offset, offset + size);
        assert_eq!(size % self.info.consts.iounit_size as usize, 0, "disk request must align to 512 bit!");
        self.get_file().write_all(&buf[..size])?;
        if self.latency {
            let delay_write = Duration::from_millis(self.info.consts.write_lat as u64);
            sleep(delay_write);
        } else {
            self.get_file().flush()?;
        }
        Ok(size)
    }

    fn ddriver_read(&mut self, buf: &mut [u8], size: usize) -> Result<usize> {
        let r = self.get_file().read(&mut buf[..size])?;
        if self.latency {
            let delay_read = Duration::from_millis(self.info.consts.read_lat as u64);
            sleep(delay_read);
        }
        Ok(r)
    }

    fn ddriver_ioctl(&mut self, cmd: u32, arg: &mut [u8]) -> Result<()> {
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

    fn ddriver_reset(&mut self) -> Result<()> {
        self.ddriver_seek(0, SeekType::Set)?;
        self.ddriver_write(&[0].repeat(self.info.consts.layout_size as usize), self.info.consts.layout_size.try_into().unwrap())?;
        // self.info = DiskInfo::default();
        Ok(())
    }

    fn ddriver_flush(&mut self) -> Result<()> {
        self.get_file().flush()?;
        Ok(())
    }

    fn ddriver_flush_range(&mut self, _left: u64, _right: u64) -> Result<()> {
        self.ddriver_flush()
    }
}

impl FileDiskDriver {
    pub fn new(path: &str, layout_size: u32, iounit_size: u32, latency: bool) -> Self {
        warn!("FileDiskDriver new, path={}, size=0x{:x}, iosz={}", path, layout_size, iounit_size);
        let mut r = Self {
            info: DiskInfo {
                stats: Default::default(),
                consts: DiskConst {
                    layout_size,
                    iounit_size,
                    ..Default::default()
                },
            },
            // file: if path.is_empty() { None } else { Some(File::open(path).unwrap()) },
            file: None,
            latency,
        };
        if !path.is_empty() {
            r.ddriver_open(path).unwrap();
        }
        r
    }
}

impl Default for FileDiskDriver {
    fn default() -> Self {
        FileDiskDriver::new("", FILE_DISK_SIZE as u32, FILE_DISK_UNIT as u32, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn simple_test() -> Result<()> {
        let mut driver = FileDiskDriver::default();
        driver_tester(&mut driver)?;
        info!("Test done.");
        Ok(())
    }
}
