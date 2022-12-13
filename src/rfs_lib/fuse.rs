/// FUSE operations.
use std::ffi::OsStr;
use std::path::Path;
use zerocopy::AsBytes;
use std::time::SystemTime;
use disk_driver::DiskDriver;
use fuser::{Filesystem, KernelConfig, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyWrite, Request, TimeOrNow};
use libc::{c_int, ENOENT};
use log::*;
use crate::rfs_lib::desc::Ext2FileType;
use crate::rfs_lib::{TTL, RFS, DEVICE_FILE};
use crate::rfs_lib::utils::*;

impl<T: DiskDriver> Filesystem for RFS<T> {
    fn init(&mut self, _req: &Request<'_>, _config: &mut KernelConfig) -> Result<(), c_int> {
        let file = DEVICE_FILE.read().unwrap().clone();
        ret(self.rfs_init(&file))
    }

    fn destroy(&mut self) {
        self.rfs_destroy().unwrap();
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        prv!("lookup", parent, name);
        rep!(reply, r, self.rfs_lookup(parent as usize, name.to_str().unwrap()));
        let (ino, inode) = r;
        let attr = inode.to_attr(ino as usize, self.block_size());
        debug!("file {} found! attr: {:?}", name.to_str().unwrap(), attr);
        reply.entry(&TTL, &attr, 0);
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        prv!("getattr", ino);
        let ino = RFS::<T>::shift_ino(ino as usize);
        rep!(reply, node, self.get_inode(ino));
        let attr = node.to_attr(ino, self.block_size());
        prv!(attr);
        reply.attr(&TTL, &attr);
    }

    fn setattr(&mut self, _req: &Request<'_>, ino: u64, mode: Option<u32>,
               uid: Option<u32>, gid: Option<u32>, size: Option<u64>,
               atime: Option<TimeOrNow>, mtime: Option<TimeOrNow>, _ctime: Option<SystemTime>,
               _fh: Option<u64>, _crtime: Option<SystemTime>, chgtime: Option<SystemTime>,
               bkuptime: Option<SystemTime>, flags: Option<u32>, reply: ReplyAttr) {
        prv!("setattr", ino, atime, mtime, size);
        rep!(reply, node, self.rfs_setattr(ino, mode, uid, gid, size,
            time_or_now_convert(atime), time_or_now_convert(mtime), chgtime, bkuptime, flags));
        let attr = node.to_attr(ino as usize, self.block_size());
        reply.attr(&TTL, &attr);
    }

    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
        rep!(reply, inode, self.get_inode(ino as usize));
        let data = inode.i_block.to_vec().as_bytes().to_vec().into_iter().collect::<Vec<u8>>();
        let mut i = 0;
        while data[i] != 0 && i < data.len() {
            i += 1;
        }
        warn!("read link: {}", String::from_utf8(data.clone()).unwrap());
        reply.data(&data[..i]);
    }

    fn mknod(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, _umask: u32, _rdev: u32, reply: ReplyEntry) {
        prv!("mknod", parent, name, mode);
        let parent = RFS::<T>::shift_ino(parent as usize);
        rep!(reply, inode_info, self.make_node(parent, name.to_str().unwrap(), mode as usize, Ext2FileType::RegularFile));
        let (ino, inode) = inode_info;
        let attr = inode.to_attr(ino, self.block_size());
        reply.entry(&TTL, &attr, 0);
        debug!("mknod done");
    }

    fn mkdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, _umask: u32, reply: ReplyEntry) {
        prv!("mkdir", parent, name, mode);
        let parent = RFS::<T>::shift_ino(parent as usize);
        rep!(reply, inode_info, self.make_node(parent, name.to_str().unwrap(), mode as usize, Ext2FileType::Directory));
        let (ino, inode) = inode_info;
        let attr = inode.to_attr(ino, self.block_size());
        reply.entry(&TTL, &attr, 0);
        debug!("mkdir done");
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        rep!(reply, self.rfs_unlink(parent as usize, name.to_str().unwrap()));
        reply.ok();
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        rep!(reply, self.rfs_rmdir(parent as usize, name.to_str().unwrap()));
        reply.ok();
    }

    fn symlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, link: &Path, reply: ReplyEntry) {
        prv!("symlink", parent, name, link);
        let link = link.to_str().unwrap();
        assert!(link.len() <= 60);
        let parent = RFS::<T>::shift_ino(parent as usize);
        rep!(reply, inode_info, self.rfs_symlink(parent, name.to_str().unwrap(), link.to_string().as_str()));
        let (ino, inode) = inode_info;
        rep!(reply, self.set_inode(ino, &inode));
        let attr = inode.to_attr(ino, self.block_size());
        reply.entry(&TTL, &attr, 0);
        debug!("symlink done");
    }

    fn rename(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, newparent: u64, newname: &OsStr, _flags: u32, reply: ReplyEmpty) {
        rep!(reply, self.rfs_rename(parent as usize, name.to_str().unwrap(), newparent as usize, newname.to_str().unwrap()));
        reply.ok();
    }

    fn read(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, size: u32,
            _flags: i32, _lock_owner: Option<u64>, reply: ReplyData) {
        prv!("read", ino, offset, size);
        rep!(reply, data, self.rfs_read(ino, offset, size));
        reply.data(&data);
    }

    fn write(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, data: &[u8],
             _write_flags: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyWrite) {
        prv!("write", ino, offset, data.len());
        rep!(reply, written, self.rfs_write(ino, offset, data));
        reply.written(written);
    }

    fn flush(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        rep!(reply, self.rfs_dump());
        reply.ok();
    }

    fn release(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _flags: i32, _lock_owner: Option<u64>, _flush: bool, reply: ReplyEmpty) {
        rep!(reply, self.rfs_dump());
        reply.ok();
    }


    fn readdir(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        prv!("readdir", ino, offset);
        rep!(reply, entries, self.rfs_readdir(ino, offset));
        for (i, d) in entries.iter().enumerate() {
            let o = i + offset as usize;
            rep!(reply, inode, self.get_inode(d.inode as usize));
            debug!("readdir entry[{}] [{}]", o, d.to_string());
            let _ = reply.add(d.inode as u64, (o + 1) as i64, inode.to_attr(d.inode as usize, self.block_size()).kind, d.get_name());
        }
        reply.ok();
    }

    fn releasedir(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _flags: i32, reply: ReplyEmpty) {
        rep!(reply, self.rfs_dump());
        reply.ok();
    }

    // fn setxattr(&mut self, _req: &Request<'_>, ino: u64, name: &OsStr, _value: &[u8], flags: i32, position: u32, reply: ReplyEmpty) {
    //     todo!()
    // }

    fn access(&mut self, _req: &Request<'_>, ino: u64, _mask: i32, reply: ReplyEmpty) {
        let ino = RFS::<T>::shift_ino(ino as usize);
        rep!(reply, self.get_inode(ino));
        reply.ok();
    }

    fn create(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, _umask: u32, _flags: i32, reply: ReplyCreate) {
        prv!("create", parent, name, mode);
        let parent = RFS::<T>::shift_ino(parent as usize);
        rep!(reply, inode_info, self.make_node(parent, name.to_str().unwrap(), mode as usize, Ext2FileType::RegularFile));
        let (ino, inode) = inode_info;
        let attr = inode.to_attr(ino, self.block_size());
        reply.created(&TTL, &attr, 0, 0, 0);
    }
}