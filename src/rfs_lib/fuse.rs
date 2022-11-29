/// FUSE operations.
use std::ffi::OsStr;
use std::time::SystemTime;
use fuse::{Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, ReplyWrite, Request};
use libc::{c_int, ENOENT};
use log::*;
use crate::rfs_lib::desc::Ext2FileType;
use crate::rfs_lib::{TTL, RFS};
use crate::rfs_lib::utils::*;

impl Filesystem for RFS {
    fn init(&mut self, _req: &Request<'_>) -> Result<(), c_int> {
        ret(self.rfs_init())
    }

    fn destroy(&mut self, _req: &Request<'_>) {
        self.rfs_destroy().unwrap();
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        prv!("lookup", parent, name);
        rep!(reply, r, self.rfs_lookup(parent as usize, name.to_str().unwrap()));
        let (ino, inode) = r;
        let attr = inode.to_attr(ino as usize);
        debug!("file {} found! attr: {:?}", name.to_str().unwrap(), attr);
        reply.entry(&TTL, &attr, 0);
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        prv!("getattr", ino);
        let ino = RFS::shift_ino(ino as usize);
        rep!(reply, node, self.get_inode(ino));
        let attr = node.to_attr(ino);
        prv!(attr);
        reply.attr(&TTL, &attr);
    }

    fn setattr(&mut self, _req: &Request<'_>, ino: u64, mode: Option<u32>,
               uid: Option<u32>, gid: Option<u32>, size: Option<u64>,
               atime: Option<SystemTime>, mtime: Option<SystemTime>, _fh: Option<u64>,
               _crtime: Option<SystemTime>, chgtime: Option<SystemTime>,
               bkuptime: Option<SystemTime>, flags: Option<u32>, reply: ReplyAttr) {
        prv!("setattr", ino, atime, mtime, size);
        rep!(reply, node, self.rfs_setattr(ino, mode, uid, gid, size, atime, mtime, chgtime, bkuptime, flags));
        let attr = node.to_attr(ino as usize);
        reply.attr(&TTL, &attr);
    }

    fn mknod(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, _rdev: u32, reply: ReplyEntry) {
        prv!("mknod", parent, name, mode);
        rep!(reply, inode_info, self.make_node(parent as usize, name.to_str().unwrap(), mode as usize, Ext2FileType::RegularFile));
        let (ino, inode) = inode_info;
        let attr = inode.to_attr(ino);
        reply.entry(&TTL, &attr, 0);
        debug!("mknod done");
    }

    fn mkdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, reply: ReplyEntry) {
        prv!("mkdir", parent, name, mode);
        rep!(reply, inode_info, self.make_node(parent as usize, name.to_str().unwrap(), mode as usize, Ext2FileType::Directory));
        let (ino, inode) = inode_info;
        let attr = inode.to_attr(ino);
        reply.entry(&TTL, &attr, 0);
        debug!("mkdir done");
    }

    fn read(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, size: u32, reply: ReplyData) {
        prv!("read", ino, offset, size);
        rep!(reply, data, self.rfs_read(ino, offset, size));
        reply.data(&data);
    }

    fn write(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, data: &[u8], _flags: u32, reply: ReplyWrite) {
        prv!("write", ino, offset, data.len());
        rep!(reply, written, self.rfs_write(ino, offset, data));
        reply.written(written);
    }

    fn readdir(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        prv!("readdir", ino, offset);
        rep!(reply, entries, self.rfs_readdir(ino, offset));
        for (i, d) in entries.iter().enumerate() {
            let o = i + offset as usize;
            rep!(reply, inode, self.get_inode(d.inode as usize));
            debug!("readdir entry[{}] [{}] {:?}", o, d.to_string(), d);
            reply.add(d.inode as u64, (o + 1) as i64, inode.to_attr(d.inode as usize).kind, d.get_name());
        }
        reply.ok();
    }
}
