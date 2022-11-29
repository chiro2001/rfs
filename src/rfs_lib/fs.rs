use std::cmp::min;
/// FUSE operations.
use std::ffi::OsStr;
use std::mem::size_of;
use std::os::raw::c_int;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::Local;
use disk_driver::{IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE};
use execute::Execute;
use fuse::{Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use libc::ENOENT;
use log::*;
use crate::{FORCE_FORMAT, prv, rep, rep_mut};
use crate::rfs_lib::desc::{EXT2_ROOT_INO, Ext2GroupDesc, Ext2INode,
                           Ext2SuperBlock, Ext2DirEntry, Ext2FileType};
use crate::rfs_lib::{TTL, RFS};
use crate::rfs_lib::utils::*;

impl Filesystem for RFS {
    fn init(&mut self, _req: &Request<'_>) -> Result<(), c_int> {
        let file = "disk";
        ret(self.driver.ddriver_open(file))?;
        // get and check size
        let mut buf = [0 as u8; 4];
        ret(self.driver.ddriver_ioctl(IOC_REQ_DEVICE_SIZE, &mut buf))?;
        self.driver_info.consts.layout_size = u32::from_be_bytes(buf.clone());
        ret(self.driver.ddriver_ioctl(IOC_REQ_DEVICE_IO_SZ, &mut buf))?;
        self.driver_info.consts.iounit_size = u32::from_be_bytes(buf.clone());
        debug!("size of super block struct is {}", size_of::<Ext2SuperBlock>());
        debug!("size of group desc struct is {}", size_of::<Ext2GroupDesc>());
        debug!("size of inode struct is {}", size_of::<Ext2INode>());

        // at lease 32 blocks
        info!("Disk {} has {} IO blocks.", file, self.driver_info.consts.disk_block_count());
        if self.disk_size() < 32 * 0x400 {
            error!("Too small disk!");
            return Err(1);
        }
        info!("disk info: {:?}", self.driver_info);
        // read super block
        let super_blk_count = size_of::<Ext2SuperBlock>() / self.disk_block_size();
        let disk_block_size = self.disk_block_size();
        info!("super block size {} disk block ({} bytes)", super_blk_count, super_blk_count * self.disk_block_size());
        let mut data_blocks_head = [0 as u8].repeat((disk_block_size * super_blk_count) as usize);
        ret(self.read_disk_blocks(&mut data_blocks_head, super_blk_count))?;
        let mut super_block: Ext2SuperBlock = unsafe { deserialize_row(&data_blocks_head) };
        if !super_block.magic_matched() {
            // maybe there is one block reserved for boot,
            // read one block again
            ret(self.read_disk_blocks(&mut data_blocks_head, super_blk_count))?;
            // data_blocks_head.reverse();
            super_block = unsafe { deserialize_row(&data_blocks_head) };
            if super_block.magic_matched() { self.filesystem_first_block = 1; }
        }
        let format = ret(FORCE_FORMAT.read())?.clone();
        if !super_block.magic_matched() || format {
            if !format { warn!("FileSystem not found! creating super block..."); } else { warn!("Will format disk!") }
            super_block = Ext2SuperBlock::default();
            // set block size to 1 KiB
            super_block.s_log_block_size = 10;
            // super block use first block (when block size is 1 KiB), set group 0 start block = 1;
            // block size bigger than 2 KiB, use 0
            super_block.s_first_data_block = if self.block_size() < 2 * 0x400 { 1 } else { 0 };
            // 4 KiB / inode
            super_block.s_inodes_count = (self.disk_size() / 0x400 / 4) as u32;
            let block_count = self.disk_size() / super_block.block_size();
            super_block.s_blocks_count = block_count as u32;
            super_block.s_free_inodes_count = super_block.s_inodes_count;
            super_block.s_free_blocks_count = super_block.s_blocks_count;

            // timestamps
            let dt = Local::now();
            super_block.s_wtime = dt.timestamp_millis() as u32;
            info!("total {} blocks", block_count);
            // TODO: create layout
            // let's use mkfs.ext2
            // use version 0
            let mut command = execute::command_args!("mkfs.ext2", file, "-t", "ext2", "-r", "0");
            command.stdout(Stdio::piped());
            let output = command.execute_output().unwrap();
            info!("{}", String::from_utf8(output.stdout).unwrap());
            // reload disk driver
            ret(self.driver.ddriver_close())?;
            ret(self.driver.ddriver_open(file))?;
            ret(self.seek_block(0))?;
            ret(self.read_disk_blocks(&mut data_blocks_head, super_blk_count))?;
            super_block = unsafe { deserialize_row(&data_blocks_head) };
            if !super_block.magic_matched() {
                ret(self.read_disk_blocks(&mut data_blocks_head, super_blk_count))?;
                super_block = unsafe { deserialize_row(&data_blocks_head) };
            }
            if super_block.magic_matched() {
                self.filesystem_first_block = 1;
                info!("Disk driver reloaded.");
            } else {
                error!("Make filesystem failed!");
                return Err(1);
            }
        } else {
            info!("FileSystem found!");
            debug!("fs: {:x?}", super_block);
        }
        self.super_block.apply_from(&super_block);
        self.print_stats();
        // read block group desc table
        debug!("first start block: {}", self.super_block.s_first_data_block);
        ret(self.seek_block(self.super_block.s_first_data_block as usize + self.filesystem_first_block))?;
        let mut data_block = self.create_block_vec();
        ret(self.read_block(&mut data_block))?;
        // just assert there is only one group now
        let group: Ext2GroupDesc = unsafe { deserialize_row(&data_block) };
        // debug!("group desc data: {:x?}", data_block);
        debug!("group: {:x?}", group);
        self.group_desc_table.push(group);

        let bg_block_bitmap = self.get_group_desc().bg_block_bitmap as usize;
        debug!("block bitmap at {} block", bg_block_bitmap);
        ret(self.seek_block(bg_block_bitmap))?;
        let mut bitmap_data_block = self.create_block_vec();
        ret(self.read_block(&mut bitmap_data_block))?;
        debug!("block bit map: {:?}", &bitmap_data_block[..32]);
        self.bitmap_data.clear();
        self.bitmap_data.extend_from_slice(&bitmap_data_block);

        let bg_inode_bitmap = self.get_group_desc().bg_inode_bitmap as usize;
        debug!("inode bitmap at {} block", bg_inode_bitmap);
        ret(self.seek_block(bg_inode_bitmap))?;
        let mut bitmap_inode = self.create_block_vec();
        ret(self.read_block(&mut bitmap_inode))?;
        debug!("inode bit map: {:?}", &bitmap_inode[..32]);
        self.bitmap_inode.clear();
        self.bitmap_inode.extend_from_slice(&bitmap_inode);

        // load root dir
        self.root_dir = ret(self.get_inode(EXT2_ROOT_INO))?;
        debug!("root dir inode: {:?}", self.root_dir);

        // let entries = ret(self.get_dir_entries())?;

        debug!("Init done.");
        Ok(())
    }

    fn destroy(&mut self, _req: &Request<'_>) {
        self.driver.ddriver_close().unwrap();
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        prv!("lookup", parent, name);
        let parent = RFS::shift_ino(parent);
        rep!(reply, entries, self.get_dir_entries(parent));
        for d in entries {
            debug!("dir entry [{}] {} type {}", d.inode, d.get_name(), d.file_type);
            if d.get_name() == name.to_str().unwrap() {
                match self.get_inode(d.inode as usize) {
                    Ok(r) => {
                        let attr = r.to_attr(d.inode as usize);
                        debug!("file {} == {} found! attr: {:?}", name.to_str().unwrap(), d.get_name(), attr);
                        reply.entry(&TTL, &attr, 0);
                        return;
                    }
                    Err(_) => {
                        reply.error(ENOENT);
                        return;
                    }
                };
            }
        }
        reply.error(ENOENT);
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        prv!("getattr", ino);
        let ino = RFS::shift_ino(ino);
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
        prv!("setattr", ino, atime, mtime);
        let ino = RFS::shift_ino(ino);
        rep_mut!(reply, node, self.get_inode(ino));
        match mode {
            Some(v) => node.i_mode = v as u16,
            _ => {}
        };
        match uid {
            Some(v) => {
                node.i_uid = (v & 0xFF) as u16;
                node.i_uid_high = (v >> 16) as u16;
            }
            _ => {}
        };
        match gid {
            Some(v) => {
                node.i_gid = (v & 0xFF) as u16;
                node.i_gid_high = (v >> 16) as u16;
            }
            _ => {}
        };
        match size {
            Some(v) => {
                node.i_size = (v & 0xFFFF) as u32;
                node.i_size_high = (v >> 32) as u32;
            }
            _ => {}
        };
        match atime {
            Some(v) => node.i_atime = v.duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            _ => {}
        };
        match mtime {
            Some(v) => node.i_mtime = v.duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            _ => {}
        };
        match chgtime {
            Some(v) => node.i_ctime = v.duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            _ => {}
        };
        match bkuptime {
            // not checked
            Some(v) => node.i_dtime = v.duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            _ => {}
        };
        match flags {
            Some(v) => node.i_flags = v,
            _ => {}
        };
        rep!(reply, self.set_inode(ino, &node));
        let attr = node.to_attr(ino);
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
        let offset = offset as usize;
        let size = size as usize;
        let sz = self.block_size();
        let ino = RFS::shift_ino(ino);
        let mut blocks: Vec<usize> = vec![];
        let start_index = offset / self.block_size();
        assert_eq!(offset % self.block_size(), 0);

        let disk_size = self.disk_size();
        let mut last_index = 0 as usize;
        let mut last_block = 0 as usize;
        // rep!(reply, self.walk_blocks_inode(ino, start_index, &mut |block, index| {
        rep!(reply, self.read_blocks_inode(ino, start_index, &mut |block, index| {
            let will_continue = (index + 1) * sz - offset < size;
            blocks.push(block);
            debug!("walk to block {} index {}, continue={}, offset now={}, size now = {}=={}",
                block, index, will_continue, (index+1) * sz, (index+1) * sz - offset, blocks.len() * sz);
            if block == 0 {
                warn!("zero block!");
                // TODO: file not found
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
            Ok(will_continue)
        }));
        let mut data: Vec<u8> = [0 as u8].repeat(size);
        for (i, block) in blocks.iter().enumerate() {
            // if i * sz >= size { break; }
            rep!(reply, self.seek_block(*block));
            let right = min((i + 1) * sz, size);
            rep!(reply, self.read_block(&mut data[(i * sz)..right]));
        }
        // rep!(reply, last_data, String::from_utf8(Vec::from(&data[data.len()-16..])));
        // debug!("last 16 byte: {}", last_data);
        reply.data(&data);
    }

    fn readdir(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        prv!("readdir", ino, offset);
        let ino = RFS::shift_ino(ino);
        rep!(reply, entries, self.get_dir_entries(ino));
        for (i, d) in entries.iter().enumerate().skip(offset as usize) {
            rep!(reply, inode, self.get_inode(d.inode as usize));
            debug!("readdir entry[{}] [{}] {:?}", i, d.to_string(), d);
            reply.add(d.inode as u64, (i + 1) as i64, inode.to_attr(d.inode as usize).kind, d.get_name());
        }
        reply.ok();
    }
}
