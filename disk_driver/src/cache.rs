use std::num::NonZeroUsize;
use anyhow::Result;
use log::{debug, warn};
use lru::LruCache;
use crate::{DiskDriver, IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE, SeekType};

#[derive(Debug, Default, Clone)]
struct CacheDiskInfo {
    size: u32,
    unit: u32,
}

#[derive(Debug, Default, Clone)]
struct CacheItem {
    valid: bool,
    data: Vec<u8>,
}

pub struct CacheDiskDriver<T: DiskDriver> {
    inner: T,
    info: CacheDiskInfo,
    // cache: LruCache<u64, CacheItem>,
    cache: LruCache<u64, Vec<u8>>,
    offset: i64,
    block_log: u64,
    size_mask: u64,
}

pub fn int_log2(a: u64) -> u64 {
    let mut t = a;
    let mut s: u64 = 0;
    while t & 0x1 == 0 {
        t >>= 1;
        s += 1;
    }
    s
}

impl<T: DiskDriver> CacheDiskDriver<T> {
    pub fn new(mut inner: T, size: usize) -> Self {
        let mut info = CacheDiskInfo::default();
        let mut buf = [0 as u8; 4];
        inner.ddriver_ioctl(IOC_REQ_DEVICE_IO_SZ, &mut buf).unwrap();
        let unit = u32::from_le_bytes(buf.clone());
        info.unit = unit.clone();
        inner.ddriver_ioctl(IOC_REQ_DEVICE_SIZE, &mut buf).unwrap();
        info.size = u32::from_le_bytes(buf.clone());
        debug!("cache init, disk size: {:x}, disk unit: {:x}", info.size, info.unit);
        Self {
            inner,
            info,
            cache: LruCache::new(NonZeroUsize::new(size).unwrap()),
            offset: 0,
            block_log: int_log2(unit as u64),
            size_mask: (1 << (int_log2(size as u64) as usize)) - 1,
        }
    }

    /// address = [ TAG | OFFSET ]
    fn get_tag(&self, address: u64) -> u64 {
        (address >> self.block_log) & self.size_mask
    }

    fn get_offset_tag(&self) -> u64 {
        self.get_tag(self.offset as u64)
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
        let unit = self.info.unit as usize;
        let unit_log = self.block_log;
        assert_eq!(0, size % unit);
        if size != unit {
            warn!("not read one disk block! size = 0x{:x}", size);
            let mut sz: usize = 0;
            for i in 0..(size >> self.block_log) {
                sz += self.ddriver_write(&buf[(i << unit_log)..((i + 1) << unit_log)], unit)?;
            }
            Ok(sz)
        } else {
            let tag = self.get_offset_tag();
            let search = self.cache.get_mut(&tag);
            match search {
                Some(data) => {
                    debug!("read hit!");
                    data.copy_from_slice(buf);
                    Ok(unit)
                }
                None => {
                    debug!("read miss!");
                    self.inner.ddriver_seek(self.offset, SeekType::Set)?;
                    let mut data = vec![0 as u8; unit];
                    // let sz = self.inner.ddriver_read(&mut data, size)?;
                    data.copy_from_slice(buf);
                    self.cache.put(tag, data);
                    self.offset += unit as i64;
                    Ok(unit)
                }
            }
        }
    }

    fn ddriver_read(&mut self, buf: &mut [u8], size: usize) -> Result<usize> {
        let unit = self.info.unit as usize;
        let unit_log = self.block_log;
        assert_eq!(0, size % unit);
        if size != unit {
            warn!("not write one disk block! size = 0x{:x}", size);
            let mut sz: usize = 0;
            for i in 0..(size >> self.block_log) {
                sz += self.ddriver_read(&mut buf[(i << unit_log)..((i + 1) << unit_log)], unit)?;
            }
            Ok(sz)
        } else {
            let tag = self.get_offset_tag();
            let search = self.cache.get(&tag);
            match search {
                Some(data) => {
                    debug!("write hit!");
                    buf.copy_from_slice(&data);
                    Ok(unit)
                }
                None => {
                    debug!("write miss!");
                    self.inner.ddriver_seek(self.offset, SeekType::Set)?;
                    let mut data = vec![0 as u8; unit];
                    let sz = self.inner.ddriver_read(&mut data, size)?;
                    buf.copy_from_slice(&data);
                    self.cache.put(tag, data);
                    self.offset += sz as i64;
                    Ok(sz)
                }
            }
        }
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
        debug!("flush cached data");
        for (tag, data) in &self.cache {
            let address = tag << self.block_log;
            self.inner.ddriver_seek(address as i64, SeekType::Set)?;
            self.inner.ddriver_write(&data, data.len())?;
        }
        self.cache.clear();
        self.inner.ddriver_flush()
    }

    fn ddriver_flush_range(&mut self, _left: u64, _right: u64) -> Result<()> {
        // self.inner.ddriver_flush_range(left, right)
        self.ddriver_flush()?;
        Ok(())
    }
}