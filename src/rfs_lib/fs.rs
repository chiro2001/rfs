use std::ffi::OsStr;
use std::mem::size_of;
use std::os::raw::c_int;
use std::process::Stdio;
use chrono::Local;
use disk_driver::{IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE};
use execute::Execute;
use fuse::{Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use libc::ENOENT;
use log::*;
use crate::{prv, rep};
use crate::rfs_lib::desc::{Ext2GroupDesc, Ext2INode, Ext2SuperBlock};
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
        if !super_block.magic_matched() {
            warn!("FileSystem not found! creating super block...");
            // let mut group_desc = Ext2GroupDesc::default();
            super_block = Ext2SuperBlock::default();
            // set block size to 1 KiB
            super_block.s_log_block_size = 10;
            // super block use first block (when block size is 1 KiB), set group 0 start block = 1;
            // block size bigger than 2 KiB, use 0
            super_block.s_first_data_block = if self.block_size() < 2 * 0x400 { 1 } else { 0 };
            // super_block.s_first_ino = 0 .. 11;
            // It can be bigger than disk... why? use default values
            // super_block.s_blocks_per_group = 8192;
            // super_block.s_clusters_per_group = 8192;
            // super_block.s_inodes_per_group = 1024;
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

    fn read(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, size: u32, reply: ReplyData) {
        prv!("read", ino, offset, size);
        debug!("#read: offset = {:x}, size = {:x}", offset, size);
        let mut offset = offset as usize;
        let ino = RFS::shift_ino(ino);
        rep!(reply, node, self.get_inode(ino));
        // debug!("to read block lists: {:x?}", node.i_block);
        let layer = self.block_size() / 4;
        let layer_layer = layer * layer;
        let layer2 = layer * 2;
        let sz = self.block_size();
        let size = size as usize;
        let block_id_capacity = sz / 4;
        assert_eq!(offset % sz, 0);
        assert_eq!(size % sz, 0);
        let max_read_blocks = size / sz;
        let mut data_blocks = self.create_blocks_vec(max_read_blocks);
        let mut data_block = vec![self.create_block_vec(); 3];
        let mut data_block_index = [usize::MAX as usize; 3];
        let mut buf_u32 = [0 as u8; 4];
        let base = offset;
        let threshold: [usize; 4] = [
            sz * 12,
            sz * (12 + layer),
            sz * (12 + layer + layer_layer),
            sz * (11 + layer + layer2 + layer_layer)];
        macro_rules! commit_data {
            () => {
                debug!("#commit {} KiB data", (offset - base) / 0x400);
                reply.data(&data_blocks[..offset - base]);
                return;
            };
        }
        macro_rules! fetch_save_data {
            ($block:expr) => {
                rep!(reply, _r, self.read_data_block($block, &mut data_blocks[offset - base..]));
            };
        }
        loop {
            if offset - base >= max_read_blocks * sz || offset >= base + size as usize {
                commit_data!();
            }
            if offset < threshold[0] {
                // block 0-11: direct addressing
                let block = node.i_block[offset / sz] as usize;
                if block == 0 { commit_data!(); }
                fetch_save_data!(block);
            } else {
                macro_rules! calc_layer {
                    ($block:expr, $l:expr, $o:expr) => {
                        {
                            let l = $l;
                            if !data_block_index[l] != $block {
                                rep!(reply, _r, self.read_data_block($block, &mut data_block[l]));
                                data_block_index[l] = $block;
                            }
                            let o = $o;
                            buf_u32.copy_from_slice(&data_block[l][o..o + 4]);
                            let block = u32::from_be_bytes(buf_u32.clone()) as usize;
                            block
                        }
                    };
                }
                if offset < threshold[1] {
                    // debug!("layer 1, offset = {:x}, size = {:x}", offset, size);
                    // layer 1
                    let block = node.i_block[12] as usize;
                    if block == 0 { commit_data!(); }
                    let block = calc_layer!(block, 0, ((offset - threshold[0]) / 4 / sz) % block_id_capacity);
                    fetch_save_data!(block);
                } else if offset < threshold[2] {
                    // debug!("layer 2");
                    // layer 2
                    let block = node.i_block[13] as usize;
                    if block == 0 { commit_data!(); }
                    let block = calc_layer!(block, 0, ((offset - threshold[1]) / 4 / 4 / sz) % block_id_capacity);
                    let block = calc_layer!(block, 1, ((offset - threshold[1]) / 4 / sz) % block_id_capacity);
                    fetch_save_data!(block);
                } else if offset < threshold[3] {
                    // debug!("layer 3");
                    // layer 3
                    let block = node.i_block[13] as usize;
                    if block == 0 { commit_data!(); }
                    let block = calc_layer!(block, 0, ((offset - threshold[2]) / 4 / 4 / 4 / sz) % block_id_capacity);
                    let block = calc_layer!(block, 1, ((offset - threshold[2]) / 4 / 4 / sz) % block_id_capacity);
                    let block = calc_layer!(block, 2, ((offset - threshold[2]) / 4 / sz) % block_id_capacity);
                    fetch_save_data!(block);
                } else {
                    // out of index
                    debug!("#ERROR");
                    reply.error(ENOENT);
                    return;
                }
            }
            offset += sz;
        }
    }

    fn readdir(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        prv!("readdir", ino, offset);
        let ino = RFS::shift_ino(ino);
        rep!(reply, entries, self.get_dir_entries(ino));
        for (i, d) in entries.iter().enumerate().skip(offset as usize) {
            rep!(reply, inode, self.get_inode(d.inode as usize));
            debug!("entry {}", d.to_string());
            reply.add(d.inode as u64, (i + 1) as i64, inode.to_attr(d.inode as usize).kind, d.get_name());
        }
        reply.ok();
    }
}
