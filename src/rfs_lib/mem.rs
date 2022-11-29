/// Manage filesystem structure in memory
use macro_tools::*;
use crate::rfs_lib::Ext2SuperBlock;

#[derive(ApplyMem, Default, Clone, Copy)]
#[ApplyMemTo(Ext2SuperBlock)]
pub struct Ext2SuperBlockMem {
    /// Inodes count
    pub s_inodes_count: u32,
    /// Reserved blocks count
    pub s_r_blocks_count: u32,
    /// Free blocks count
    pub s_free_blocks_count: u32,
    /// Free inodes count
    pub s_free_inodes_count: u32,
    /// First Data Block
    pub s_first_data_block: u32,
    /// Block size
    pub s_log_block_size: u32,

    /// First non-reserved inode
    pub s_first_ino: u32,
}

impl Ext2SuperBlockMem {
    pub fn to_string(&self) -> String {
        format!("EXT2 {} inodes, {} KiB per block, free inodes {}, free blocks {}",
                self.s_inodes_count, 1 << self.s_log_block_size,
                self.s_free_inodes_count, self.s_free_blocks_count)
    }
}