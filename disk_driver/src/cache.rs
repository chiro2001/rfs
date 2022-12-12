use anyhow::Result;
use crate::{DiskDriver, SeekType};

pub struct CacheDiskDriver<T: DiskDriver> {
    inner: T,
}

impl<T: DiskDriver> CacheDiskDriver<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: DiskDriver> DiskDriver for CacheDiskDriver<T> {
    fn ddriver_open(&mut self, path: &str) -> Result<()> {
        self.inner.ddriver_open(path)
    }

    fn ddriver_close(&mut self) -> Result<()> {
        self.inner.ddriver_close()
    }

    fn ddriver_seek(&mut self, offset: i64, whence: SeekType) -> Result<u64> {
        self.inner.ddriver_seek(offset, whence)
    }

    fn ddriver_write(&mut self, buf: &[u8], size: usize) -> Result<usize> {
        self.inner.ddriver_write(buf, size)
    }

    fn ddriver_read(&mut self, buf: &mut [u8], size: usize) -> Result<usize> {
        self.inner.ddriver_read(buf, size)
    }

    fn ddriver_ioctl(&mut self, cmd: u32, arg: &mut [u8]) -> Result<()> {
        self.inner.ddriver_ioctl(cmd, arg)
    }

    fn ddriver_reset(&mut self) -> Result<()> {
        self.inner.ddriver_reset()
    }

    fn ddriver_flush(&mut self) -> Result<()> {
        self.inner.ddriver_flush()
    }

    fn ddriver_flush_range(&mut self, left: u64, right: u64) -> Result<()> {
        // self.inner.ddriver_flush_range(left, right)
        Ok(())
    }
}