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

#[derive(Debug, Default, Clone, PartialOrd, PartialEq, Ord, Eq)]
struct CacheItem {
    dirty: bool,
    data: Vec<u8>,
}

/// Test LRU:
/// ```rust
/// use lru::LruCache;
/// use std::num::NonZeroUsize;
/// let mut cache = LruCache::<usize, usize>::new(NonZeroUsize::new(2).unwrap());
/// let tag = 0x114514 as usize;
/// let raw_data = 0xa as usize;
/// cache.push(tag, raw_data);
/// cache.push(tag + 1, raw_data + 1);
/// // cache.push(tag + 2, raw_data + 2);
/// let data = cache.get_mut(&tag).unwrap();
/// *data = 0xb;
/// let data = cache.get(&tag).unwrap();
/// assert_eq!(*data, 0xb);
/// ```
pub struct CacheDiskDriver<T: DiskDriver> {
    inner: T,
    info: CacheDiskInfo,
    cache: LruCache<u64, CacheItem>,
    offset: i64,
    block_log: u64,
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

pub fn show_hex_debug(data: &[u8], group_size: usize) {
    let mut v = vec![];
    for (i, b) in data.iter().enumerate() {
        v.push(*b);
        if i % group_size == group_size - 1 || i == data.len() - 1 {
            debug!("{}", v.iter().map(|x| format!("{:2x}", x)).collect::<Vec<_>>().join(" "));
            v.clear();
        }
    }
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
        let block_log = int_log2(unit as u64);
        assert_eq!(1 << block_log, unit);
        let cache = LruCache::new(NonZeroUsize::new(size).unwrap());
        debug!("cache init, cache size: {}, disk size: {:x}, disk unit: {:x}; block_log: {}",
            size, info.size, info.unit, block_log);
        Self { inner, info, cache, offset: 0, block_log }
    }

    /// address = [ TAG | OFFSET ]
    fn get_tag(&self, address: u64) -> u64 {
        address >> self.block_log
        // address / self.info.unit as u64
    }

    fn get_offset_tag(&self) -> u64 {
        self.get_tag(self.offset as u64)
    }

    fn write_back_item(&mut self, replaced: Option<(u64, CacheItem)>) -> Result<()> {
        match replaced {
            Some((tag, item)) => {
                if item.dirty {
                    let address = tag << self.block_log;
                    // let address = tag * self.info.unit as u64;
                    debug!("cache write back to {:x}", address);
                    let unit = self.info.unit as usize;
                    self.inner.ddriver_seek(address as i64, SeekType::Set)?;
                    self.inner.ddriver_write(&item.data, unit)?;
                }
            }
            None => {}
        };
        Ok(())
    }
}

impl<T: DiskDriver> DiskDriver for CacheDiskDriver<T> {
    fn ddriver_open(&mut self, path: &str) -> Result<()> {
        self.cache.clear();
        self.inner.ddriver_open(path)
    }

    fn ddriver_close(&mut self) -> Result<()> {
        self.ddriver_flush()?;
        self.inner.ddriver_close()
    }

    fn ddriver_seek(&mut self, offset: i64, whence: SeekType) -> Result<u64> {
        // if whence == SeekType::Set {
        //     debug!("cache seek to {:x}", offset);
        // }
        match whence {
            SeekType::Set => self.offset = offset,
            SeekType::Cur => self.offset += offset,
            SeekType::End => self.offset = self.info.size as i64 - offset,
        };
        // self.inner.ddriver_seek(offset, whence)?;
        // what's meaning?
        Ok(self.offset as u64)
    }

    fn ddriver_write(&mut self, buf: &[u8], size: usize) -> Result<usize> {
        let unit = self.info.unit as usize;
        let unit_log = self.block_log;
        assert_eq!(0, size % unit);
        // debug!("cache writing data at {:x}, size: {:x}:", self.offset, size);
        // show_hex_debug(&buf[..0x20], 0x10);
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
            // debug!("cache search tag: {:x}", tag);
            match search {
                Some(item) => {
                    // debug!("write hit!");
                    item.data.copy_from_slice(buf);
                    item.dirty = true;
                    // debug!("write updated:");
                    // show_hex_debug(&item.data[..0x20], 0x10);
                    self.offset += unit as i64;
                    Ok(unit)
                }
                None => {
                    // debug!("write miss!");
                    let mut data = vec![0 as u8; unit];
                    // do not need to read again, new write will cover
                    data.copy_from_slice(buf);
                    // debug!("write newed:");
                    // show_hex_debug(&data[..0x20], 0x10);
                    let replaced = self.cache.push(tag, CacheItem { data, dirty: true });
                    self.write_back_item(replaced)?;
                    self.offset += unit as i64;
                    Ok(unit)
                }
            }
            // self.inner.ddriver_seek(self.offset, SeekType::Set)?;
            // let sz = self.inner.ddriver_write(buf, size)?;
            // self.offset += sz as i64;
            // Ok(sz)
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
            // debug!("cache search tag: {:x}", tag);
            match search {
                Some(item) => {
                    // debug!("read hit!");
                    buf.copy_from_slice(&item.data);
                    // show_hex_debug(&item.data[..0x20], 0x10);
                    self.offset += unit as i64;
                    Ok(unit)
                }
                None => {
                    // debug!("read miss!");
                    self.inner.ddriver_seek(self.offset, SeekType::Set)?;
                    let mut data = vec![0 as u8; unit];
                    let sz = self.inner.ddriver_read(&mut data, size)?;
                    buf.copy_from_slice(&data);
                    // show_hex_debug(&data[..0x20], 0x10);
                    let replaced = self.cache.push(tag, CacheItem { data, dirty: false });
                    self.write_back_item(replaced)?;
                    self.offset += sz as i64;
                    Ok(sz)
                }
            }
            // self.inner.ddriver_seek(self.offset, SeekType::Set)?;
            // let sz = self.inner.ddriver_read(buf, size)?;
            // self.offset += sz as i64;
            // Ok(sz)
        }
    }

    fn ddriver_ioctl(&mut self, cmd: u32, arg: &mut [u8]) -> Result<()> {
        self.inner.ddriver_ioctl(cmd, arg)
    }

    fn ddriver_reset(&mut self) -> Result<()> {
        self.ddriver_flush()?;
        self.inner.ddriver_reset()?;
        Ok(())
    }

    fn ddriver_flush(&mut self) -> Result<()> {
        debug!("flush cached data");
        for (tag, item) in &self.cache {
            if !item.dirty { continue; }
            let address = tag << self.block_log;
            self.inner.ddriver_seek(address as i64, SeekType::Set)?;
            // show_hex_debug(&item.data[..0x20], 0x10);
            self.inner.ddriver_write(&item.data, item.data.len())?;
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