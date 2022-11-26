use macro_tools::*;

#[derive(apply_mem)]
pub struct Ext2SuperBlockMem {
    /* Inodes count */
    pub s_inodes_count: u32,
    /* Reserved blocks count */
    pub s_r_blocks_count: u32,
    /* Free blocks count */
    pub s_free_blocks_count: u32,
    /* Free inodes count */
    pub s_free_inodes_count: u32,
    /* First Data Block */
    pub s_first_data_block: u32,
    /* Block size */
    pub s_log_block_size: u32,
}

