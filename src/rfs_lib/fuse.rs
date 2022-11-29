use std::cmp::min;
/// FUSE operations.
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::mem::size_of;
use std::os::raw::c_int;
use std::path::Path;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use disk_driver::{IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE};
use execute::Execute;
use fuse::{Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, ReplyWrite, Request};
use libc::ENOENT;
use log::*;
use crate::{DEVICE_FILE, FORCE_FORMAT, MKFS_FORMAT, prv, rep, rep_mut};
// use crate::rfs_lib::fs::RustFileSystem;
use crate::rfs_lib::desc::{EXT2_ROOT_INO, Ext2GroupDesc, Ext2INode,
                           Ext2SuperBlock, Ext2FileType, FsLayoutArgs};
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
        debug!("#read: offset = {:x}, size = {:x}", offset, size);
        let mut offset = offset as usize;
        let size = size as usize;
        let sz = self.block_size();
        let ino = RFS::shift_ino(ino as usize);
        let mut blocks: Vec<usize> = vec![];
        let start_index = offset / self.block_size();
        assert_eq!(offset % self.block_size(), 0);

        let disk_size = self.disk_size();
        let mut last_index = 0 as usize;
        let mut last_block = 0 as usize;
        // rep!(reply, self.walk_blocks_inode(ino, start_index, &mut |block, index| {
        rep!(reply, self.visit_blocks_inode(ino, start_index, &mut |block, index| {
            let will_continue = (index + 1) * sz - offset < size;
            blocks.push(block);
            debug!("read walk to block {} index {}, continue={}, offset now={}, size now = {}=={}",
                block, index, will_continue, (index+1) * sz, (index+1) * sz - offset, blocks.len() * sz);
            if block == 0 {
                debug!("zero block!");
                return Ok((will_continue, false));
            }
            if block * sz > disk_size {
                panic!("error block number {:x}!", block);
            }
            // Ok((index + 1 - start_index) * sz < size)
            if last_index != 0 && last_index + 1 != index {
                panic!("error index increase! index now: {}", index);
            }
            last_index = index;
            if last_block != 0 && last_block > block {
                error!("error block increase! block now: {}, last block: {}", block, last_block);
            }
            last_block = block;
            Ok((will_continue, false))
        }));
        let mut data: Vec<u8> = [0 as u8].repeat(size);
        for (i, block) in blocks.iter().enumerate() {
            // if i * sz >= size { break; }
            let right = min((i + 1) * sz, size);
            rep!(reply, self.read_data_block(*block, &mut data[(i * sz)..right]));
            offset += right - (i * sz);
        }
        // rep!(reply, last_data, String::from_utf8(Vec::from(&data[data.len()-16..])));
        // debug!("last 16 byte: {}", last_data);
        reply.data(&data);
    }

    fn write(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, data: &[u8], _flags: u32, reply: ReplyWrite) {
        let size = data.len() as usize;
        prv!("write", ino, offset, size);
        debug!("#write: offset = {:x}, size = {:x}", offset, size);
        let mut offset = offset as usize;
        let base = offset;
        let sz = self.block_size();
        let ino = RFS::shift_ino(ino as usize);
        let start_index = offset as usize / self.block_size();
        assert_eq!(offset % self.block_size(), 0);

        let mut blocks: Vec<usize> = vec![];

        let disk_size = self.disk_size();
        let mut last_index = 0 as usize;
        let mut last_block = 0 as usize;
        assert_eq!(0, offset % sz);
        // rep!(reply, self.walk_blocks_inode(ino, start_index, &mut |block, index| {
        rep!(reply, self.visit_blocks_inode(ino, start_index, &mut |block, index| {
            let will_continue = (index + 1) * sz - offset < size;
            debug!("write walk to block {} index {}, continue={}, offset now={}, size now = {}, size total = {}",
                block, index, will_continue, (index+1) * sz, (index+1) * sz - offset, size);
            if block == 0 {
                debug!("zero block!");
                return Ok((will_continue, index * sz - offset < size));
            }
            blocks.push(block);
            if block * sz > disk_size {
                panic!("error block number {:x}!", block);
            }
            // Ok((index + 1 - start_index) * sz < size)
            if last_index != 0 && last_index + 1 != index {
                panic!("error index increase! index now: {}", index);
            }
            last_index = index;
            if last_block != 0 && last_block > block {
                error!("error block increase! block now: {}, last block: {}", block, last_block);
            }
            last_block = block;
            Ok((will_continue, false))
        }));
        for (i, block) in blocks.iter().enumerate() {
            // if i * sz >= size { break; }
            let right = min((i + 1) * sz, size);
            rep!(reply, self.write_data_block(*block, &data[(i * sz)..right]));
            offset += right - (i * sz);
        }
        debug!("update file stats");
        rep_mut!(reply, inode, self.get_inode(ino));
        let filesize = inode.i_size as i64 | ((inode.i_size_high as i64) << 32);
        if offset as i64 > filesize {
            // TODO: large file
            inode.i_size = offset as u32;
            inode.i_size_high = (offset >> 32) as u32;
            rep!(reply, self.set_inode(ino, &inode));
        }
        let written = offset - base;
        debug!("#write: reply written = {}", written);
        reply.written(written as u32);
    }

    fn readdir(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        prv!("readdir", ino, offset);
        let ino = RFS::shift_ino(ino as usize);
        rep!(reply, entries, self.get_dir_entries(ino));
        for (i, d) in entries.iter().enumerate().skip(offset as usize) {
            rep!(reply, inode, self.get_inode(d.inode as usize));
            debug!("readdir entry[{}] [{}] {:?}", i, d.to_string(), d);
            reply.add(d.inode as u64, (i + 1) as i64, inode.to_attr(d.inode as usize).kind, d.get_name());
        }
        reply.ok();
    }
}
