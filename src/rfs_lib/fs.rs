use std::ffi::OsStr;
use std::mem::size_of;
use std::os::raw::c_int;
use std::process::Stdio;
use chrono::Local;
use disk_driver::{IOC_REQ_DEVICE_IO_SZ, IOC_REQ_DEVICE_SIZE};
use execute::Execute;
use fuse::{Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use libc::ENOENT;
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
        println!("size of super block struct is {}", size_of::<Ext2SuperBlock>());
        println!("size of group desc struct is {}", size_of::<Ext2GroupDesc>());
        println!("size of inode struct is {}", size_of::<Ext2INode>());

        // at lease 32 blocks
        println!("Disk {} has {} IO blocks.", file, self.driver_info.consts.disk_block_count());
        if self.disk_size() < 32 * 0x400 {
            println!("Too small disk!");
            return Err(1);
        }
        println!("disk info: {:?}", self.driver_info);
        // read super block
        let super_blk_count = size_of::<Ext2SuperBlock>() / self.disk_block_size();
        let disk_block_size = self.disk_block_size();
        println!("super block size {} disk block ({} bytes)", super_blk_count, super_blk_count * self.disk_block_size());
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
            println!("FileSystem not found! creating super block...");
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
            println!("total {} blocks", block_count);
            // TODO: create layout
            // let's use mkfs.ext2
            // use version 0
            let mut command = execute::command_args!("mkfs.ext2", file, "-t", "ext2", "-r", "0");
            command.stdout(Stdio::piped());
            let output = command.execute_output().unwrap();
            println!("{}", String::from_utf8(output.stdout).unwrap());
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
                println!("Disk driver reloaded.");
            } else {
                println!("Make filesystem failed!");
                return Err(1);
            }
        } else {
            println!("FileSystem found!");
            println!("fs: {:x?}", super_block);
        }
        self.super_block.apply_from(&super_block);
        // println!("s_log_block_size = {}", super_block.s_log_block_size);
        self.print_stats();
        // read block group desc table
        println!("first start block: {}", self.super_block.s_first_data_block);
        ret(self.seek_block(self.super_block.s_first_data_block as usize + self.filesystem_first_block))?;
        let mut data_block = self.create_block_vec();
        ret(self.read_block(&mut data_block))?;
        // just assert there is only one group now
        let group: Ext2GroupDesc = unsafe { deserialize_row(&data_block) };
        // println!("group desc data: {:x?}", data_block);
        println!("group: {:x?}", group);
        self.group_desc_table.push(group);
        // let bg_block_bitmap = self.get_group_desc().bg_block_bitmap as usize;

        // println!("block bitmap at {} block", bg_block_bitmap);
        // ret(self.seek_block(bg_block_bitmap))?;
        // let mut bitmap_data_block = self.create_block_vec();
        // ret(self.read_block(&mut bitmap_data_block))?;
        // println!("block bit map: {:?}", &bitmap_data_block[..32]);
        //
        // let bg_inode_bitmap = self.get_group_desc().bg_inode_bitmap as usize;
        // println!("inode bitmap at {} block", bg_inode_bitmap);
        // ret(self.seek_block(bg_inode_bitmap))?;
        // let mut bitmap_inode = self.create_block_vec();
        // ret(self.read_block(&mut bitmap_inode))?;
        // println!("inode bit map: {:?}", &bitmap_inode[..32]);
        //
        // let inode_table_n = 4 as usize;
        // let bg_inode_table = self.get_group_desc().bg_inode_table as usize;
        // println!("inode table start at {} block", bg_inode_table);
        // ret(self.seek_block(bg_inode_table))?;
        // let mut bg_inode_table = self.create_blocks_vec(inode_table_n);
        // ret(self.read_blocks(&mut bg_inode_table, inode_table_n))?;
        // println!("inode table: {:?}", &bg_inode_table[..32]);
        // let inode_table: Vec<Ext2INode> = (0..(bg_inode_table.len() / size_of::<Ext2INode>())).map(|index| {
        //     unsafe { deserialize_row(&bg_inode_table[(index * size_of::<Ext2INode>())..]) }
        // }).collect();
        // let inode = &inode_table[self.super_block.s_first_ino as usize + 1];
        // println!("first inode table is [{}+1]: {:?}", self.super_block.s_first_ino, inode);
        // println!("pointing to blocks: {:x?}", inode.i_block);
        // let inode = ret(self.get_inode(self.super_block.s_first_ino as usize + 1))?;
        // println!("got inode table: {:x?}", inode);
        // // println!("block [13] is {:x}, ")
        //
        // let inode_root = ret(self.get_inode(EXT2_ROOT_INO))?;
        // prv!(inode_root);
        //
        // let block_id = inode_root.i_block[0] as usize;
        // prv!(block_id);
        //
        // let data_block = ret(self.get_data_block(block_id))?;
        // prv!(&data_block[..64]);
        //
        // prv!(EXT2_DIR_ENTRY_BASE_SIZE);
        // prv!(size_of::<char>());
        // let mut p = 0;
        // let mut dirs = vec![];
        // while p <= data_block.len() {
        //     let dir: Ext2DirEntry = unsafe { deserialize_row(&data_block[p..]) };
        //     if dir.name_len == 0 { break; }
        //     println!("[p {:x}] name_len = {}", p, dir.name_len);
        //     // align p to word
        //     p += EXT2_DIR_ENTRY_BASE_SIZE + dir.name_len as usize;
        //     let inc = p & 0x3;
        //     p &= !0x3;
        //     if inc != 0 { p += 0x4; }
        //     println!("next p: {:x}", p);
        //     // println!("dir {:?}", dir);
        //     dirs.push(dir);
        // }
        //
        // for d in dirs {
        //     println!("dir {}", d.to_string());
        // }
        //
        // let dirs = ret(self.get_dirs(EXT2_ROOT_INO))?;
        // for d in &dirs {
        //     println!("ROOT/{}", d.to_string());
        // }
        // let dir = &dirs[2];
        // prv!(dir);
        // let dirs2 = ret(self.get_dirs(dir.inode as usize))?;
        // for d in &dirs2 {
        //     println!("{}/{}", dir.get_name(), d.to_string());
        // }

        println!("Init done.");
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
            println!("dir entry [{}] {} type {}", d.inode, d.get_name(), d.file_type);
            if d.get_name() == name.to_str().unwrap() {
                match self.get_inode(d.inode as usize) {
                    Ok(r) => {
                        let attr = r.to_attr(d.inode as usize);
                        println!("file {} == {} found! attr: {:?}", name.to_str().unwrap(), d.get_name(), attr);
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
        let ino = RFS::shift_ino(ino);
        rep!(reply, node, self.get_inode(ino));
    }

    fn readdir(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        prv!("readdir", ino, offset);
        let ino = RFS::shift_ino(ino);
        rep!(reply, entries, self.get_dir_entries(ino));
        for (i, d) in entries.iter().enumerate().skip(offset as usize) {
            rep!(reply, inode, self.get_inode(d.inode as usize));
            println!("entry {}", d.to_string());
            reply.add(d.inode as u64, (i + 1) as i64, inode.to_attr(d.inode as usize).kind, d.get_name());
        }
        reply.ok();
    }
}
