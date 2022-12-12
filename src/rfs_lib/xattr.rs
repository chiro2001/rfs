//! Ext2 filesystem SPEC
//! see: https://www.nongnu.org/ext2-doc/ext2.html
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(non_upper_case_globals)]

/*
  File: linux/ext2_ext_attr.h

  On-disk format of extended attributes for the ext2 filesystem.

  (C) 2000 Andreas Gruenbacher, <a.gruenbacher@computer.org>
*/

/* Magic value in attribute blocks */
pub const EXT2_EXT_ATTR_MAGIC_v1: usize = 0xEA010000;
pub const EXT2_EXT_ATTR_MAGIC: usize = 0xEA020000;

/* Maximum number of references to one attribute block */
pub const EXT2_EXT_ATTR_REFCOUNT_MAX: usize = 1024;

struct Ext2ExtAttrHeader {
    pub h_magic: u32,	/* magic number for identification */
    pub h_refcount: u32,	/* reference count */
    pub h_blocks: u32,	/* number of disk blocks used */
    pub h_hash: u32,		/* hash value of all attributes */
    pub h_checksum: u32,	/* crc32c(uuid+id+xattrs) */
    /* id = inum if refcount = 1, else blknum */
    pub h_reserved: [u32; 3],	/* zero right now */
}

struct Ext2ExtAttrEntry {
    pub e_name_len: u8,	/* length of name */
    pub e_name_index: u8,	/* attribute name index */
    pub e_value_offs: u16,	/* offset in disk block of value */
    pub e_value_inum: u32,	/* inode in which the value is stored */
    pub e_value_size: u32,	/* size of attribute value */
    pub e_hash: u32,		/* hash value of name and value */
    // #if 0
    // char	e_name[0];	/* attribute name */
    // #endif
}

pub const EXT2_EXT_ATTR_PAD_BITS: usize = 2;
pub const EXT2_EXT_ATTR_PAD: usize = 1usize << EXT2_EXT_ATTR_PAD_BITS;
pub const EXT2_EXT_ATTR_ROUND: usize = EXT2_EXT_ATTR_PAD - 1;