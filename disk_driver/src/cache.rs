use std::num::NonZeroUsize;
use anyhow::Result;
use log::debug;
use lru::LruCache;
use crate::{DiskDriver, IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE, SeekType};

#[derive(Debug, Default)]
struct CacheDiskInfo {
    size: u32,
    unit: u32,
}

pub struct CacheDiskDriver<T: DiskDriver> {
    inner: T,
    info: CacheDiskInfo,
    cache: LruCache<u32, Vec<u8>>,
    offset: i64,
}

impl<T: DiskDriver> CacheDiskDriver<T> {
    pub fn new(mut inner: T, size: usize) -> Self {
        let mut info = CacheDiskInfo::default();
        let mut buf = [0 as u8; 4];
        inner.ddriver_ioctl(IOC_REQ_DEVICE_IO_SZ, &mut buf).unwrap();
        info.unit = u32::from_le_bytes(buf.clone());
        inner.ddriver_ioctl(IOC_REQ_DEVICE_SIZE, &mut buf).unwrap();
        info.size = u32::from_le_bytes(buf.clone());
        debug!("cache init, disk size: {:x}, disk unit: {:x}", info.size, info.unit);
        Self {
            inner,
            info,
            cache: LruCache::new(NonZeroUsize::new(size).unwrap()),
            offset: 0,
        }
    }
}

impl<T: DiskDriver> DiskDriver for CacheDiskDriver<T> {
    fn ddriver_open(&mut self, path: &str) -> Result<()> {
        self.inner.ddriver_open(path)
    }

    fn ddriver_close(&mut self) -> Result<()> {
        self.ddriver_flush()?;
        self.inner.ddriver_close()
    }

    fn ddriver_seek(&mut self, offset: i64, whence: SeekType) -> Result<u64> {
        // self.inner.ddriver_seek(offset, whence)
        match whence {
            SeekType::Set => self.offset = offset,
            SeekType::Cur => self.offset += offset,
            SeekType::End => self.offset = self.info.size as i64 - offset,
        };
        // what's meaning?
        Ok(self.offset as u64)
    }

    fn ddriver_write(&mut self, buf: &[u8], size: usize) -> Result<usize> {
        assert_eq!(0, size % self.info.unit as usize);
        self.inner.ddriver_seek(self.offset, SeekType::Set)?;
        let sz = self.inner.ddriver_write(buf, size)?;
        self.offset += sz as i64;
        Ok(sz)
    }

    fn ddriver_read(&mut self, buf: &mut [u8], size: usize) -> Result<usize> {
        assert_eq!(0, size % self.info.unit as usize);
        self.inner.ddriver_seek(self.offset, SeekType::Set)?;
        let sz = self.inner.ddriver_read(buf, size)?;
        self.offset += sz as i64;
        Ok(sz)
    }

    fn ddriver_ioctl(&mut self, cmd: u32, arg: &mut [u8]) -> Result<()> {
        self.inner.ddriver_ioctl(cmd, arg)
    }

    fn ddriver_reset(&mut self) -> Result<()> {
        self.inner.ddriver_reset()?;
        self.ddriver_flush()?;
        Ok(())
    }

    fn ddriver_flush(&mut self) -> Result<()> {
        // TODO: dump cached data
        self.cache.clear();
        self.inner.ddriver_flush()
    }

    fn ddriver_flush_range(&mut self, _left: u64, _right: u64) -> Result<()> {
        // self.inner.ddriver_flush_range(left, right)
        self.ddriver_flush()?;
        Ok(())
    }
}