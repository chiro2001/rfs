//! Ext2 filesystem SPEC
//! see: https://www.nongnu.org/ext2-doc/ext2.html
#![allow(dead_code)]
#![allow(unused_variables)]

/**
 * Define EXT2_PREALLOCATE to preallocate data blocks for expanding files
 */
use std::mem::size_of;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, NaiveDateTime, Utc};
use fuser::{FileAttr, FileType};
use log::debug;
use rand::Rng;
use crate::prv;
use crate::rfs_lib::types::{le16, le32, s16};
use crate::rfs_lib::utils::up_align;

pub const EXT2_DEFAULT_PREALLOC_BLOCKS: usize = 8;

/**
 * The second extended file system version
 */
pub const EXT2FS_DATE: &'static str = "95/08/09";
pub const EXT2FS_VERSION: &'static str = "0.5b";

/**
 * Special inode numbers
 */
///   Bad blocks inode 
pub const EXT2_BAD_INO: usize = 1;
///   Root inode 
pub const EXT2_ROOT_INO: usize = 2;
///   User quota inode 
pub const EXT4_USR_QUOTA_INO: usize = 3;
///   Group quota inode 
pub const EXT4_GRP_QUOTA_INO: usize = 4;
///   Boot loader inode 
pub const EXT2_BOOT_LOADER_INO: usize = 5;
///   Undelete directory inode 
pub const EXT2_UNDEL_DIR_INO: usize = 6;
///   Reserved group descriptors inode 
pub const EXT2_RESIZE_INO: usize = 7;
///   Journal inode 
pub const EXT2_JOURNAL_INO: usize = 8;
///   The "exclude" inode, for snapshots 
pub const EXT2_EXCLUDE_INO: usize = 9;
///   Used by non-upstream feature 
pub const EXT4_REPLICA_INO: usize = 10;

/* First non-reserved inode for old ext2 filesystems */
pub const EXT2_GOOD_OLD_FIRST_INO: usize = 11;

/**
 * The second extended file system magic number
 */
pub const EXT2_SUPER_MAGIC: u16 = 0xEF53;
/**
 * Maximal count of links to a file
 */
pub const EXT2_LINK_MAX: usize = 65000;

/**
 * ACL structures
 */
struct Ext2AclHeader /* Header of Access Control Lists */
{
    pub aclh_size: u32,
    pub aclh_file_count: u32,
    pub aclh_acle_count: u32,
    pub aclh_first_acle: u32,
}

struct Ext2AclEntry /* Access Control List Entry */
{
    pub acle_size: u32,
    ///   Access permissions 
    pub acle_perms: u16,
    ///   Type of entry 
    pub acle_type: u16,
    ///   User or group identity 
    pub acle_tag: u16,
    pub acle_pad1: u16,
    ///   Pointer on next entry for the 
    pub acle_next: u32,
    /* same inode or on next free entry */
}

/**
 * Structure of a blocks group descriptor
 */
#[derive(Debug, Copy, Clone)]
#[repr(C, align(2))]
pub struct Ext2GroupDesc {
    ///   Blocks bitmap block 
    pub bg_block_bitmap: u32,
    ///   Inodes bitmap block 
    pub bg_inode_bitmap: u32,
    ///   Inodes table block 
    pub bg_inode_table: u32,
    ///   Free blocks count 
    pub bg_free_blocks_count: u16,
    ///   Free inodes count 
    pub bg_free_inodes_count: u16,
    ///   Directories count 
    pub bg_used_dirs_count: u16,
    pub bg_flags: u16,
    ///   Exclude bitmap for snapshots 
    pub bg_exclude_bitmap_lo: u32,
    ///   crc32c(s_uuid+grp_num+bitmap) LSB 
    pub bg_block_bitmap_csum_lo: u16,
    ///   crc32c(s_uuid+grp_num+bitmap) LSB 
    pub bg_inode_bitmap_csum_lo: u16,
    ///   Unused inodes count 
    pub bg_itable_unused: u16,
    ///   crc16(s_uuid+group_num+group_desc)
    pub bg_checksum: u16,
}

impl Default for Ext2GroupDesc {
    fn default() -> Self {
        Self {
            bg_block_bitmap: 3,
            bg_inode_bitmap: 4,
            bg_inode_table: 5,
            bg_free_blocks_count: 0xf6e,
            bg_free_inodes_count: 0x3f5,
            bg_used_dirs_count: 2,
            bg_flags: 4,
            bg_exclude_bitmap_lo: 0,
            bg_block_bitmap_csum_lo: 0,
            bg_inode_bitmap_csum_lo: 0,
            bg_itable_unused: 0,
            bg_checksum: 0,
        }
    }
}

impl From<FsLayoutArgs> for Ext2GroupDesc {
    fn from(l: FsLayoutArgs) -> Self {
        Self {
            bg_inode_bitmap: l.inode_map as u32,
            bg_block_bitmap: l.data_map as u32,
            bg_inode_table: l.inode_table as u32,
            bg_free_blocks_count: l.block_count as u16,
            bg_free_inodes_count: l.inode_count as u16,
            bg_used_dirs_count: 0,
            ..Self::default()
        }
    }
}

///   Inode table/bitmap not initialized 
pub const EXT2_BG_INODE_UNINIT: usize = 0x0001;
///   Block bitmap not initialized 
pub const EXT2_BG_BLOCK_UNINIT: usize = 0x0002;
///   On-disk itable initialized to zero 
pub const EXT2_BG_INODE_ZEROED: usize = 0x0004;

/**
 * Data structures used by the directory indexing feature
 *
 * Note: all of the multibyte integer fields are little endian.
 */

/**
 * Note: dx_root_info is laid out so that if it should somehow get
 * overlaid by a dirent the two low bits of the hash version will be
 * zero.  Therefore, the hash version mod 4 should never be 0.
 * Sincerely, the paranoia department.
 */
// struct ext2_dx_root_info {
//     pub reserved_zero: u32,
//     ///   0 now, 1 at release 
//     pub hash_version: u8,
//     ///   8 
//     pub info_length: u8,
//     pub indirect_levels: u8,
//     pub unused_flags: u8,
// };


///   reserved for userspace lib 
pub const EXT2_HASH_LEGACY: usize = 0;
pub const EXT2_HASH_HALF_MD4: usize = 1;
pub const EXT2_HASH_TEA: usize = 2;
pub const EXT2_HASH_LEGACY_UNSIGNED: usize = 3;
///   reserved for userspace lib 
pub const EXT2_HASH_HALF_MD4_UNSIGNED: usize = 4;
///   reserved for userspace lib 
pub const EXT2_HASH_TEA_UNSIGNED: usize = 5;
pub const EXT2_HASH_SIPHASH: usize = 6;

pub const EXT2_HASH_FLAG_INCOMPAT: usize = 0x1;

pub const EXT4_DX_BLOCK_MASK: usize = 0x0fffffff;

/**
struct ext2_dx_entry {
  pub hash: le32,
  pub block: le32,
};

struct ext2_dx_countlimit {
  pub limit: le16,
  pub count: le16,
};
 */

/**
 * Constants relative to the data blocks
 */
pub const EXT2_NDIR_BLOCKS: usize = 12;
pub const EXT2_IND_BLOCK: usize = EXT2_NDIR_BLOCKS;
pub const EXT2_DIND_BLOCK: usize = EXT2_IND_BLOCK + 1;
pub const EXT2_TIND_BLOCK: usize = EXT2_DIND_BLOCK + 1;
pub const EXT2_N_BLOCKS: usize = EXT2_TIND_BLOCK + 1;

/**
 * Inode flags
 */
///   Secure deletion 
pub const EXT2_SECRM_FL: usize = 0x00000001;
///   Undelete 
pub const EXT2_UNRM_FL: usize = 0x00000002;
///   Compress file 
pub const EXT2_COMPR_FL: usize = 0x00000004;
///   Synchronous updates 
pub const EXT2_SYNC_FL: usize = 0x00000008;
///   Immutable file 
pub const EXT2_IMMUTABLE_FL: usize = 0x00000010;
///   writes to file may only append 
pub const EXT2_APPEND_FL: usize = 0x00000020;
///   do not dump file 
pub const EXT2_NODUMP_FL: usize = 0x00000040;
///   do not update atime 
pub const EXT2_NOATIME_FL: usize = 0x00000080;
/* Reserved for compression usage... */
///   One or more compressed clusters 
pub const EXT2_DIRTY_FL: usize = 0x00000100;
pub const EXT2_COMPRBLK_FL: usize = 0x00000200;
///   Access raw compressed data 
pub const EXT2_NOCOMPR_FL: usize = 0x00000400;
/* nb: was previously EXT2_ECOMPR_FL */
///   encrypted inode 
pub const EXT4_ENCRYPT_FL: usize = 0x00000800;
/* End compression flags --- maybe not all used */
///   btree format dir 
pub const EXT2_BTREE_FL: usize = 0x00001000;
///   hash-indexed directory 
pub const EXT2_INDEX_FL: usize = 0x00001000;
///   file data should be journaled 
pub const EXT2_IMAGIC_FL: usize = 0x00002000;
pub const EXT3_JOURNAL_DATA_FL: usize = 0x00004000;
///   file tail should not be merged 
pub const EXT2_NOTAIL_FL: usize = 0x00008000;
///   Synchronous directory modifications 
pub const EXT2_DIRSYNC_FL: usize = 0x00010000;
///   Top of directory hierarchies
pub const EXT2_TOPDIR_FL: usize = 0x00020000;
///   Set to each huge file 
pub const EXT4_HUGE_FILE_FL: usize = 0x00040000;
///   Inode uses extents 
pub const EXT4_EXTENTS_FL: usize = 0x00080000;
///   Verity protected inode 
pub const EXT4_VERITY_FL: usize = 0x00100000;
///   Inode used for large EA 
pub const EXT4_EA_INODE_FL: usize = 0x00200000;
/* EXT4_EOFBLOCKS_FL 0x00400000 was here */
///   Do not cow file 
pub const FS_NOCOW_FL: usize = 0x00800000;
///   Inode is a snapshot 
pub const EXT4_SNAPFILE_FL: usize = 0x01000000;
///   Inode is DAX 
pub const FS_DAX_FL: usize = 0x02000000;
///   Snapshot is being deleted 
pub const EXT4_SNAPFILE_DELETED_FL: usize = 0x04000000;
///   Snapshot shrink has completed 
pub const EXT4_SNAPFILE_SHRUNK_FL: usize = 0x08000000;
///   Inode has inline data 
pub const EXT4_INLINE_DATA_FL: usize = 0x10000000;
///   Create with parents projid 
pub const EXT4_PROJINHERIT_FL: usize = 0x20000000;
///   Casefolded file 
pub const EXT4_CASEFOLD_FL: usize = 0x40000000;
///   reserved for ext2 lib 
pub const EXT2_RESERVED_FL: usize = 0x80000000;

///   User visible flags 
pub const EXT2_FL_USER_VISIBLE: usize = 0x604BDFFF;
///   User modifiable flags 
pub const EXT2_FL_USER_MODIFIABLE: usize = 0x604B80FF;

#[derive(Debug, Clone)]
#[repr(C, align(2))]
pub struct Ext2INode {
    /*00*/ ///   File mode
    pub i_mode: u16,
    ///   Low 16 bits of Owner Uid 
    pub i_uid: u16,
    ///   Size in bytes 
    pub i_size: u32,
    ///   Access time 
    pub i_atime: u32,
    ///   Inode change time 
    pub i_ctime: u32,
    /*10*/ ///   Modification time 
    pub i_mtime: u32,
    ///   Deletion Time 
    pub i_dtime: u32,
    ///   Low 16 bits of Group Id 
    pub i_gid: u16,
    ///   Links count 
    pub i_links_count: u16,
    ///   Blocks count 
    pub i_blocks: u32,
    /*20*/ ///   File flags 
    pub i_flags: u32,
    ///   was l_i_reserved1 
    pub i_version: u32,
    /*28*/ ///   Pointers to blocks 
    pub i_block: [u32; EXT2_N_BLOCKS],
    /*64*/ ///   File version (for NFS) 
    pub i_generation: u32,
    ///   File ACL 
    pub i_file_acl: u32,
    pub i_size_high: u32,
    /*70*/ ///   Fragment address 
    pub i_faddr: u32,
    pub i_blocks_hi: u16,
    pub i_file_acl_high: u16,
    ///   these 2 fields    
    pub i_uid_high: u16,
    ///   were reserved2[0] 
    pub i_gid_high: u16,
    ///   crc32c(uuid+inum+inode) 
    pub i_checksum_lo: u16,
    pub i_reserved: u16,
}

pub const EXT2_INODE_SIZE: usize = size_of::<Ext2INode>();

pub fn utc_time(timestamp_seconds: u32) -> SystemTime {
    let naive = NaiveDateTime::from_timestamp_millis(timestamp_seconds as i64 * 1000).unwrap();
    let datetime: DateTime<Utc> = DateTime::from_utc(naive, Utc);
    SystemTime::from(datetime)
}

pub fn get_time_now() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32
}

use num_enum::{TryFromPrimitive, IntoPrimitive};

#[derive(Debug, Clone, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(usize)]
pub enum Ext2FileType {
    /// Unknown
    Unknown = 0,
    /// Regular file (S_IFREG)
    RegularFile = 8,
    /// Directory (S_IFDIR)
    Directory = 4,
    /// Character device (S_IFCHR)
    CharDevice = 2,
    /// Block device (S_IFBLK)
    BlockDevice = 6,
    /// Named pipe (S_IFIFO)
    NamedPipe = 1,
    /// Unix domain socket (S_IFSOCK)
    Socket = 0xc,
    /// Symbolic link (S_IFLNK)
    Symlink = 0xa,
}

impl Ext2INode {
    pub fn to_attr(&self, ino: usize, blksize: usize) -> FileAttr {
        prv!("to_attr", ino, self);
        let kind = match self.i_mode >> 12 {
            0x1 => FileType::NamedPipe,
            0x2 => FileType::CharDevice,
            0x4 => FileType::Directory,
            0x6 => FileType::BlockDevice,
            0xa => FileType::Symlink,
            0xc => FileType::Socket,
            // Default to regular file, which is 0x8
            _ => FileType::RegularFile,
        };
        let perm = self.i_mode & 0xFFF;
        prv!(self.i_mode, kind, perm);
        FileAttr {
            ino: ino as u64,
            size: self.i_size as u64,
            blocks: self.i_blocks as u64,
            atime: utc_time(self.i_atime),
            mtime: utc_time(self.i_mtime),
            ctime: utc_time(self.i_ctime),
            // Time of creation (macOS only)
            crtime: UNIX_EPOCH,
            // high 4 bits: file format
            kind,
            // low 12 bits: use/group and access rights
            perm,
            nlink: self.i_links_count as u32,
            uid: self.i_uid as u32 + (self.i_uid_high as u32) << 16,
            gid: self.i_gid as u32 + (self.i_uid_high as u32) << 16,
            rdev: 0,
            blksize: blksize as u32,
            flags: 0,
        }
    }
}

impl Default for Ext2INode {
    fn default() -> Self {
        Self {
            i_mode: 0,
            i_uid: 0,
            i_size: 0,
            i_atime: get_time_now(),
            i_ctime: get_time_now(),
            i_mtime: get_time_now(),
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 0,
            i_blocks: 0,
            i_flags: 0,
            i_version: 0,
            i_block: [0; 15],
            i_generation: 0,
            i_file_acl: 0,
            i_size_high: 0,
            i_faddr: 0,
            i_blocks_hi: 0,
            i_file_acl_high: 0,
            i_uid_high: 0,
            i_gid_high: 0,
            i_checksum_lo: 0,
            i_reserved: 0,
        }
    }
}

/**
 * File system states
 */
///   Unmounted cleanly 
pub const EXT2_VALID_FS: usize = 0x0001;
///   Errors detected 
pub const EXT2_ERROR_FS: usize = 0x0002;
///   Orphans being recovered 
pub const EXT3_ORPHAN_FS: usize = 0x0004;
///   Ext4 fast commit replay ongoing 
pub const EXT4_FC_REPLAY: usize = 0x0020;

/**
 * Misc. filesystem flags
 */
///   Signed dirhash in use 
pub const EXT2_FLAGS_SIGNED_HASH: usize = 0x0001;
///   Unsigned dirhash in use 
pub const EXT2_FLAGS_UNSIGNED_HASH: usize = 0x0002;
///   OK for use on development code 
pub const EXT2_FLAGS_TEST_FILESYS: usize = 0x0004;
///   This is a snapshot image 
pub const EXT2_FLAGS_IS_SNAPSHOT: usize = 0x0010;
///   Snapshot inodes corrupted 
pub const EXT2_FLAGS_FIX_SNAPSHOT: usize = 0x0020;
///   Exclude bitmaps corrupted 
pub const EXT2_FLAGS_FIX_EXCLUDE: usize = 0x0040;

/**
 * Mount flags
 */
///   Do mount-time checks 
pub const EXT2_MOUNT_CHECK: usize = 0x0001;
///   Create files with directory's group 
pub const EXT2_MOUNT_GRPID: usize = 0x0004;
///   Some debugging messages 
pub const EXT2_MOUNT_DEBUG: usize = 0x0008;
///   Continue on errors 
pub const EXT2_MOUNT_ERRORS_CONT: usize = 0x0010;
///   Remount fs ro on errors 
pub const EXT2_MOUNT_ERRORS_RO: usize = 0x0020;
///   Panic on errors 
pub const EXT2_MOUNT_ERRORS_PANIC: usize = 0x0040;
///   Mimics the Minix statfs 
pub const EXT2_MOUNT_MINIX_DF: usize = 0x0080;
///   Disable 32-bit UIDs 
pub const EXT2_MOUNT_NO_UID32: usize = 0x0200;

/**
 * Maximal mount counts between two filesystem checks
 */
///   Allow 20 mounts 
pub const EXT2_DFL_MAX_MNT_COUNT: usize = 20;
///   Don't use interval check 
pub const EXT2_DFL_CHECKINTERVAL: usize = 0;

/**
 * Behaviour when detecting errors
 */
///   Continue execution 
pub const EXT2_ERRORS_CONTINUE: usize = 1;
///   Remount fs read-only 
pub const EXT2_ERRORS_RO: usize = 2;
///   Panic 
pub const EXT2_ERRORS_PANIC: usize = 3;
pub const EXT2_ERRORS_DEFAULT: usize = EXT2_ERRORS_CONTINUE;

/* Metadata checksum algorithms */
pub const EXT2_CRC32C_CHKSUM: usize = 1;

/* Encryption algorithms, key size and key reference len */
pub const EXT4_ENCRYPTION_MODE_INVALID: usize = 0;
pub const EXT4_ENCRYPTION_MODE_AES_256_XTS: usize = 1;
pub const EXT4_ENCRYPTION_MODE_AES_256_GCM: usize = 2;
pub const EXT4_ENCRYPTION_MODE_AES_256_CBC: usize = 3;
pub const EXT4_ENCRYPTION_MODE_AES_256_CTS: usize = 4;

pub const EXT4_AES_256_XTS_KEY_SIZE: usize = 64;
pub const EXT4_AES_256_GCM_KEY_SIZE: usize = 32;
pub const EXT4_AES_256_CBC_KEY_SIZE: usize = 32;
pub const EXT4_AES_256_CTS_KEY_SIZE: usize = 32;
pub const EXT4_MAX_KEY_SIZE: usize = 64;

pub const EXT4_KEY_DESCRIPTOR_SIZE: usize = 8;
pub const EXT4_CRYPTO_BLOCK_SIZE: usize = 16;

/* Password derivation constants */
pub const EXT4_MAX_PASSPHRASE_SIZE: usize = 1024;
pub const EXT4_MAX_SALT_SIZE: usize = 256;
pub const EXT4_PBKDF2_ITERATIONS: usize = 0xFFFF;

pub const EXT2_LABEL_LEN: usize = 16;

/**
 * Structure of the super block
 */
#[derive(Debug)]
#[repr(C, align(2))]
pub struct Ext2SuperBlock {
    /*000*/ ///   Inodes count 
    pub s_inodes_count: u32,
    ///   Blocks count 
    pub s_blocks_count: u32,
    ///   Reserved blocks count 
    pub s_r_blocks_count: u32,
    ///   Free blocks count 
    pub s_free_blocks_count: u32,
    /*010*/ ///   Free inodes count 
    pub s_free_inodes_count: u32,
    ///   First Data Block 
    pub s_first_data_block: u32,
    ///   Block size 
    pub s_log_block_size: u32,
    ///   Allocation cluster size 
    pub s_log_cluster_size: u32,
    /*020*/ ///   # Blocks per group 
    pub s_blocks_per_group: u32,
    ///   # Fragments per group 
    pub s_clusters_per_group: u32,
    ///   # Inodes per group 
    pub s_inodes_per_group: u32,
    ///   Mount time 
    pub s_mtime: u32,
    /*030*/ ///   Write time 
    pub s_wtime: u32,
    ///   Mount count 
    pub s_mnt_count: u16,
    ///   Maximal mount count 
    pub s_max_mnt_count: s16,
    ///   Magic signature 
    pub s_magic: u16,
    ///   File system state 
    pub s_state: u16,
    ///   Behaviour when detecting errors 
    pub s_errors: u16,
    ///   minor revision level 
    pub s_minor_rev_level: u16,
    /*040*/ ///   time of last check 
    pub s_lastcheck: u32,
    ///   max. time between checks 
    pub s_checkinterval: u32,
    ///   OS 
    pub s_creator_os: u32,
    ///   Revision level 
    pub s_rev_level: u32,
    /*050*/ ///   Default uid for reserved blocks 
    pub s_def_resuid: u16,
    ///   Default gid for reserved blocks 
    pub s_def_resgid: u16,
    /**
     * These fields are for EXT2_DYNAMIC_REV superblocks only.
     *
     * Note: the difference between the compatible feature set and
     * the incompatible feature set is that if there is a bit set
     * in the incompatible feature set that the kernel doesn't
     * know about, it should refuse to mount the filesystem.
     *
     * e2fsck's requirements are more strict; if it doesn't know
     * about a feature in either the compatible or incompatible
     * feature set, it must abort and not try to meddle with
     * things it doesn't understand...
     */
    ///   First non-reserved inode 
    pub s_first_ino: u32,
    ///   size of inode structure 
    pub s_inode_size: u16,
    ///   block group # of this superblock 
    pub s_block_group_nr: u16,
    ///   compatible feature set 
    pub s_feature_compat: u32,
    /*060*/ ///   incompatible feature set 
    pub s_feature_incompat: u32,
    ///   readonly-compatible feature set 
    pub s_feature_ro_compat: u32,
    /*068*/ ///   128-bit uuid for volume 
    pub s_uuid: [u8; 16],
    /*078*/ ///   volume name, no NUL? 
    pub s_volume_name: [u8; EXT2_LABEL_LEN],
    /*088*/ ///   directory last mounted on, no NUL? 
    pub s_last_mounted: [u8; 64],
    /*0c8*/ ///   For compression 
    pub s_algorithm_usage_bitmap: u32,
    /**
     * Performance hints.  Directory preallocation should only
     * happen if the EXT2_FEATURE_COMPAT_DIR_PREALLOC flag is on.
     */
    ///   Nr of blocks to try to preallocate
    pub s_prealloc_blocks: u8,
    ///   Nr to preallocate for dirs 
    pub s_prealloc_dir_blocks: u8,
    ///   Per group table for online growth 
    pub s_reserved_gdt_blocks: u16,
    /**
     * Journaling support valid if EXT2_FEATURE_COMPAT_HAS_JOURNAL set.
     */
    /*0d0*/ ///   uuid of journal superblock 
    pub s_journal_uuid: [u8; 16],
    /*0e0*/ ///   inode number of journal file 
    pub s_journal_inum: u32,
    ///   device number of journal file 
    pub s_journal_dev: u32,
    ///   start of list of inodes to delete 
    pub s_last_orphan: u32,
    /*0ec*/ ///   HTREE hash seed 
    pub s_hash_seed: [u32; 4],
    /*0fc*/ ///   Default hash version to use 
    pub s_def_hash_version: u8,
    ///   Default type of journal backup 
    pub s_jnl_backup_type: u8,
    ///   Group desc. size: INCOMPAT_64BIT 
    pub s_desc_size: u16,
    /**
     * Other options
     */
    /*100*/ ///   default EXT2_MOUNT_* flags used 
    pub s_default_mount_opts: u32,
    ///   First metablock group 
    pub s_first_meta_bg: u32,
    ///   When the filesystem was created 
    pub s_mkfs_time: u32,
    /*10c*/ ///   Backup of the journal inode 
    pub s_jnl_blocks: [u32; 17],
    /*150*/ ///   Blocks count high 32bits 
    pub s_blocks_count_hi: u32,
    ///   Reserved blocks count high 32 bits
    pub s_r_blocks_count_hi: u32,
    ///   Free blocks count 
    pub s_free_blocks_hi: u32,
    ///   All inodes have at least # bytes 
    pub s_min_extra_isize: u16,
    ///   New inodes should reserve # bytes 
    pub s_want_extra_isize: u16,
    /*160*/ ///   Miscellaneous flags 
    pub s_flags: u32,
    ///   RAID stride in blocks 
    pub s_raid_stride: u16,
    ///   # seconds to wait in MMP checking 
    pub s_mmp_update_interval: u16,
    ///   Block for multi-mount protection 
    pub s_mmp_block: u64,
    /*170*/ ///   blocks on all data disks (N*stride)
    pub s_raid_stripe_width: u32,
    ///   FLEX_BG group size 
    pub s_log_groups_per_flex: u8,
    ///   metadata checksum algorithm 
    pub s_checksum_type: u8,
    ///   versioning level for encryption 
    pub s_encryption_level: u8,
    ///   Padding to next 32bits 
    pub s_reserved_pad: u8,
    ///   nr of lifetime kilobytes written 
    pub s_kbytes_written: u64,
    /*180*/ ///   Inode number of active snapshot 
    pub s_snapshot_inum: u32,
    ///   sequential ID of active snapshot 
    pub s_snapshot_id: u32,
    ///   active snapshot reserved blocks 
    pub s_snapshot_r_blocks_count: u64,
    /*190*/ ///   inode number of disk snapshot list 
    pub s_snapshot_list: u32,
    // pub const EXT4_S_ERR_START: usize = ext4_offsetof;(struct Ext2SuperBlock, s_error_count)
    ///   number of fs errors 
    pub s_error_count: u32,
    ///   first time an error happened 
    pub s_first_error_time: u32,
    ///   inode involved in first error 
    pub s_first_error_ino: u32,
    /*1a0*/ ///   block involved in first error 
    pub s_first_error_block: u64,
    ///   function where error hit, no NUL?
    pub s_first_error_func: [u8; 32],
    /*1c8*/ ///   line number where error happened 
    pub s_first_error_line: u32,
    ///   most recent time of an error 
    pub s_last_error_time: u32,
    /*1d0*/ ///   inode involved in last error 
    pub s_last_error_ino: u32,
    ///   line number where error happened 
    pub s_last_error_line: u32,
    ///   block involved of last error 
    pub s_last_error_block: u64,
    /*1e0*/ ///   function where error hit, no NUL? 
    pub s_last_error_func: [u8; 32],
    // pub const EXT4_S_ERR_END: usize = ext4_offsetof;(struct Ext2SuperBlock, s_mount_opts)
    /*200*/ ///   default mount options, no NUL? 
    pub s_mount_opts: [u8; 64],
    /*240*/ ///   inode number of user quota file 
    pub s_usr_quota_inum: u32,
    ///   inode number of group quota file 
    pub s_grp_quota_inum: u32,
    ///   overhead blocks/clusters in fs 
    pub s_overhead_clusters: u32,
    /*24c*/ ///   If sparse_super2 enabled 
    pub s_backup_bgs: [u32; 2],
    /*254*/ ///   Encryption algorithms in use  
    pub s_encrypt_algos: [u8; 4],
    /*258*/ ///   Salt used for string2key algorithm 
    pub s_encrypt_pw_salt: [u8; 16],
    /*268*/ ///   Location of the lost+found inode 
    pub s_lpf_ino: le32,
    ///   inode for tracking project quota 
    pub s_prj_quota_inum: le32,
    /*270*/ ///   crc32c(orig_uuid) if csum_seed set 
    pub s_checksum_seed: le32,
    /*274*/ pub s_wtime_hi: u8,
    pub s_mtime_hi: u8,
    pub s_mkfs_time_hi: u8,
    pub s_lastcheck_hi: u8,
    pub s_first_error_time_hi: u8,
    pub s_last_error_time_hi: u8,
    pub s_first_error_errcode: u8,
    pub s_last_error_errcode: u8,
    /*27c*/ ///   Filename charset encoding 
    pub s_encoding: le16,
    ///   Filename charset encoding flags 
    pub s_encoding_flags: le16,
    ///   Padding to the end of the block 
    pub s_reserved: [le32; 95],
    /*3fc*/ ///   crc32c(superblock) 
    pub s_checksum: u32,
}

pub fn create_uuid() -> [u8; 16] {
    let mut rng = rand::thread_rng();
    (0..16).map(|_| { rng.gen::<u8>() }).collect::<Vec<u8>>().try_into().unwrap()
}

impl Ext2SuperBlock {
    pub fn new(s_inodes_count: u32, s_blocks_count: u32, s_first_data_block: u32,
               s_log_block_size: u32) -> Self {
        Self {
            s_inodes_count,
            s_blocks_count,
            s_first_data_block,
            s_log_block_size,
            s_inodes_per_group: s_inodes_count,
            ..Self::default()
        }
    }
}

/// This struct records offsets
#[derive(Debug, Default, Clone)]
pub struct FsLayoutArgs {
    pub block_count: usize,
    pub block_size: usize,
    // if has boot block
    pub boot: bool,
    // offsets
    pub super_block: usize,
    pub group_desc: usize,
    pub data_map: usize,
    pub inode_map: usize,
    pub inode_table: usize,
    pub inode_count: usize,
}

impl From<FsLayoutArgs> for Ext2SuperBlock {
    fn from(l: FsLayoutArgs) -> Self {
        let mut r =
            Self::new(l.inode_count as u32, l.block_count as u32,
                      if l.block_size < 2 * 0x400 { 1 } else { 0 },
                      match l.block_size {
                          1024 => 0,
                          2048 => 1,
                          4096 => 2,
                          _ => panic!("unsupported block size")
                      });
        r.s_free_blocks_count = (l.block_count - 1 - 1 - 1 - 1) as u32;
        r.s_free_inodes_count = (l.inode_count -
            (1 + 1 + 1 + 1 + 1 + l.inode_count / size_of::<Ext2INode>() + 1)
        ) as u32;
        r
    }
}

impl Default for Ext2SuperBlock {
    fn default() -> Self {
        Self {
            s_inodes_count: 1024,
            s_blocks_count: 4096,
            s_r_blocks_count: 204,
            s_free_blocks_count: 3806,
            s_free_inodes_count: 1013,
            s_first_data_block: 1,
            s_log_block_size: 0,
            s_log_cluster_size: 0,
            s_blocks_per_group: 8192,
            s_clusters_per_group: 8192,
            s_inodes_per_group: 1024,
            s_mtime: 0,
            s_wtime: get_time_now(),
            s_mnt_count: 0,
            s_max_mnt_count: 65535,
            s_magic: EXT2_SUPER_MAGIC,
            s_state: 1,
            s_errors: 1,
            s_minor_rev_level: 0,
            s_lastcheck: get_time_now(),
            s_checkinterval: 0,
            s_creator_os: 0,
            s_rev_level: 1,
            s_def_resuid: 0,
            s_def_resgid: 0,
            s_first_ino: 11,
            s_inode_size: size_of::<Ext2INode>() as u16,
            s_block_group_nr: 0,
            s_feature_compat: 56,
            s_feature_incompat: 2,
            s_feature_ro_compat: 3,
            s_uuid: create_uuid(),
            s_volume_name: [0; EXT2_LABEL_LEN],
            s_last_mounted: [0; 64],
            s_algorithm_usage_bitmap: 0,
            s_prealloc_blocks: 0,
            s_prealloc_dir_blocks: 0,
            s_reserved_gdt_blocks: 15,
            s_journal_uuid: [0; 16],
            s_journal_inum: 0,
            s_journal_dev: 0,
            s_last_orphan: 0,
            s_hash_seed: [3087838277, 2185897224, 2377460875, 2234914617],
            s_def_hash_version: 1,
            s_jnl_backup_type: 0,
            s_desc_size: 0,
            s_default_mount_opts: 12,
            s_first_meta_bg: 0,
            s_mkfs_time: get_time_now(),
            s_jnl_blocks: [0; 17],
            s_blocks_count_hi: 0,
            s_r_blocks_count_hi: 0,
            s_free_blocks_hi: 0,
            s_min_extra_isize: 32,
            s_want_extra_isize: 32,
            s_flags: 1,
            s_raid_stride: 0,
            s_mmp_update_interval: 0,
            s_mmp_block: 0,
            s_raid_stripe_width: 0,
            s_log_groups_per_flex: 0,
            s_checksum_type: 0,
            s_encryption_level: 0,
            s_reserved_pad: 0,
            s_kbytes_written: 0,
            s_snapshot_inum: 0,
            s_snapshot_id: 0,
            s_snapshot_r_blocks_count: 0,
            s_snapshot_list: 0,
            s_error_count: 0,
            s_first_error_time: 0,
            s_first_error_ino: 0,
            s_first_error_block: 0,
            s_first_error_func: [0; 32],
            s_first_error_line: 0,
            s_last_error_time: 0,
            s_last_error_ino: 0,
            s_last_error_line: 0,
            s_last_error_block: 0,
            s_last_error_func: [0; 32],
            s_mount_opts: [0; 64],
            s_usr_quota_inum: 0,
            s_grp_quota_inum: 0,
            s_overhead_clusters: 276,
            s_backup_bgs: [0; 2],
            s_encrypt_algos: [0; 4],
            s_encrypt_pw_salt: [0; 16],
            s_lpf_ino: 0,
            s_prj_quota_inum: 0,
            s_checksum_seed: 0,
            s_wtime_hi: 0,
            s_mtime_hi: 0,
            s_mkfs_time_hi: 0,
            s_lastcheck_hi: 0,
            s_first_error_time_hi: 0,
            s_last_error_time_hi: 0,
            s_first_error_errcode: 0,
            s_last_error_errcode: 0,
            s_encoding: 0,
            s_encoding_flags: 0,
            s_reserved: [0; 95],
            s_checksum: 0,
        }
    }
}

impl Ext2SuperBlock {
    pub fn magic_matched(&self) -> bool { self.s_magic == EXT2_SUPER_MAGIC }
    pub fn block_size(&self) -> usize { self.block_size_kib() * 0x400 }
    pub fn block_size_kib(&self) -> usize { 1 << self.s_log_block_size }
}

/**
 * Codes for operating systems
 */
pub const EXT2_OS_LINUX: usize = 0;
pub const EXT2_OS_HURD: usize = 1;
pub const EXT2_OBSO_OS_MASIX: usize = 2;
pub const EXT2_OS_FREEBSD: usize = 3;
pub const EXT2_OS_LITES: usize = 4;

/**
 * Revision levels
 */
///   The good old (original) format 
pub const EXT2_GOOD_OLD_REV: usize = 0;
///   V2 format w/ dynamic inode sizes 
pub const EXT2_DYNAMIC_REV: usize = 1;

pub const EXT2_CURRENT_REV: usize = EXT2_GOOD_OLD_REV;
pub const EXT2_MAX_SUPP_REV: usize = EXT2_DYNAMIC_REV;

pub const EXT2_GOOD_OLD_INODE_SIZE: usize = 128;

/**
 * Journal inode backup types
 */
pub const EXT3_JNL_BACKUP_BLOCKS: usize = 1;

pub const EXT2_FEATURE_COMPAT_DIR_PREALLOC: usize = 0x0001;
pub const EXT2_FEATURE_COMPAT_IMAGIC_INODES: usize = 0x0002;
pub const EXT3_FEATURE_COMPAT_HAS_JOURNAL: usize = 0x0004;
pub const EXT2_FEATURE_COMPAT_EXT_ATTR: usize = 0x0008;
pub const EXT2_FEATURE_COMPAT_RESIZE_INODE: usize = 0x0010;
pub const EXT2_FEATURE_COMPAT_DIR_INDEX: usize = 0x0020;
pub const EXT2_FEATURE_COMPAT_LAZY_BG: usize = 0x0040;
/* #define EXT2_FEATURE_COMPAT_EXCLUDE_INODE	0x0080 not used, legacy */
pub const EXT2_FEATURE_COMPAT_EXCLUDE_BITMAP: usize = 0x0100;
pub const EXT4_FEATURE_COMPAT_SPARSE_SUPER2: usize = 0x0200;
pub const EXT4_FEATURE_COMPAT_FAST_COMMIT: usize = 0x0400;
pub const EXT4_FEATURE_COMPAT_STABLE_INODES: usize = 0x0800;

pub const EXT2_FEATURE_RO_COMPAT_SPARSE_SUPER: usize = 0x0001;
pub const EXT2_FEATURE_RO_COMPAT_LARGE_FILE: usize = 0x0002;
/* #define EXT2_FEATURE_RO_COMPAT_BTREE_DIR	0x0004 not used */
pub const EXT4_FEATURE_RO_COMPAT_HUGE_FILE: usize = 0x0008;
pub const EXT4_FEATURE_RO_COMPAT_GDT_CSUM: usize = 0x0010;
pub const EXT4_FEATURE_RO_COMPAT_DIR_NLINK: usize = 0x0020;
pub const EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE: usize = 0x0040;
pub const EXT4_FEATURE_RO_COMPAT_HAS_SNAPSHOT: usize = 0x0080;
pub const EXT4_FEATURE_RO_COMPAT_QUOTA: usize = 0x0100;
pub const EXT4_FEATURE_RO_COMPAT_BIGALLOC: usize = 0x0200;
/**
 * METADATA_CSUM implies GDT_CSUM.  When METADATA_CSUM is set, group
 * descriptor checksums use the same algorithm as all other data
 * structures' checksums.
 */
///   Project quota 
pub const EXT4_FEATURE_RO_COMPAT_METADATA_CSUM: usize = 0x0400;
pub const EXT4_FEATURE_RO_COMPAT_REPLICA: usize = 0x0800;
pub const EXT4_FEATURE_RO_COMPAT_READONLY: usize = 0x1000;
pub const EXT4_FEATURE_RO_COMPAT_PROJECT: usize = 0x2000;
///   Needs recovery 
pub const EXT4_FEATURE_RO_COMPAT_SHARED_BLOCKS: usize = 0x4000;
pub const EXT4_FEATURE_RO_COMPAT_VERITY: usize = 0x8000;

pub const EXT2_FEATURE_INCOMPAT_COMPRESSION: usize = 0x0001;
pub const EXT2_FEATURE_INCOMPAT_FILETYPE: usize = 0x0002;
pub const EXT3_FEATURE_INCOMPAT_RECOVER: usize = 0x0004;
///   Journal device 
pub const EXT3_FEATURE_INCOMPAT_JOURNAL_DEV: usize = 0x0008;
///   >2GB or 3-lvl htree 
pub const EXT2_FEATURE_INCOMPAT_META_BG: usize = 0x0010;
pub const EXT3_FEATURE_INCOMPAT_EXTENTS: usize = 0x0040;
pub const EXT4_FEATURE_INCOMPAT_64BIT: usize = 0x0080;
pub const EXT4_FEATURE_INCOMPAT_MMP: usize = 0x0100;
pub const EXT4_FEATURE_INCOMPAT_FLEX_BG: usize = 0x0200;
pub const EXT4_FEATURE_INCOMPAT_EA_INODE: usize = 0x0400;
pub const EXT4_FEATURE_INCOMPAT_DIRDATA: usize = 0x1000;
pub const EXT4_FEATURE_INCOMPAT_CSUM_SEED: usize = 0x2000;
pub const EXT4_FEATURE_INCOMPAT_LARGEDIR: usize = 0x4000;
///   data in inode 
pub const EXT4_FEATURE_INCOMPAT_INLINE_DATA: usize = 0x8000;
pub const EXT4_FEATURE_INCOMPAT_ENCRYPT: usize = 0x10000;
pub const EXT4_FEATURE_INCOMPAT_CASEFOLD: usize = 0x20000;

pub const EXT2_FEATURE_COMPAT_SUPP: usize = 0;
pub const EXT2_FEATURE_INCOMPAT_SUPP: usize =
    EXT2_FEATURE_INCOMPAT_FILETYPE | EXT4_FEATURE_INCOMPAT_MMP |
        EXT4_FEATURE_INCOMPAT_LARGEDIR | EXT4_FEATURE_INCOMPAT_EA_INODE;
pub const EXT2_FEATURE_RO_COMPAT_SUPP: usize =
    EXT2_FEATURE_RO_COMPAT_SPARSE_SUPER | EXT2_FEATURE_RO_COMPAT_LARGE_FILE |
        EXT4_FEATURE_RO_COMPAT_DIR_NLINK | /*EXT2_FEATURE_RO_COMPAT_BTREE_DIR |*/
        EXT4_FEATURE_RO_COMPAT_VERITY;

/**
 * Structure of a directory entry
 */
pub const EXT2_NAME_LEN: usize = 255;

#[derive(Debug, Clone, Copy)]
#[repr(C, align(1))]
pub struct Ext2DirEntry {
    ///   Inode number 
    pub inode: u32,
    ///   Directory entry length 
    pub rec_len: u16,
    // ///   Name length 
    // pub name_len: u16,
    // temporally use deprecated structure, for having no logic from cpp
    ///   Name length 
    pub name_len: u8,
    ///   File type
    pub file_type: u8,
    ///   File name 
    pub name: [u8; EXT2_NAME_LEN],
}

impl Default for Ext2DirEntry {
    fn default() -> Self {
        Self {
            inode: 0,
            rec_len: 0,
            name_len: 0,
            file_type: 0,
            name: [0; EXT2_NAME_LEN],
        }
    }
}

pub const EXT2_DIR_ENTRY_BASE_SIZE: usize = size_of::<Ext2DirEntry>() - EXT2_NAME_LEN;

impl Ext2DirEntry {
    pub fn get_name(&self) -> String {
        String::from_utf8_lossy(&self.name[..self.name_len as usize]).to_string()
    }
    pub fn to_string(&self) -> String {
        format!("{} {} entry size {} name size {}", self.inode,
                self.get_name(), self.rec_len, self.name_len)
    }
    fn name_len(&self) -> usize {
        for (i, v) in self.name.iter().enumerate() {
            if *v == 0 { return i; }
        };
        return self.name.len();
    }
    pub fn update_rec_len(&mut self) {
        self.rec_len = up_align(4 + 2 + 1 + 1 + self.name_len(), 2) as u16;
        debug!("update_rec_len: {}", self.rec_len);
    }
    pub fn update_name(&mut self, name: &str) {
        let name_bytes = name.as_bytes();
        assert!(name_bytes.len() < EXT2_NAME_LEN);
        self.name[..name_bytes.len()].copy_from_slice(name_bytes);
        self.name_len = name_bytes.len() as u8;
        assert!(name.len() < 256, "Too long filename!");
        self.update_rec_len();
    }
    pub fn new(name: &str, inode: usize, file_type: u8) -> Self {
        let mut e =
            Self {
                inode: inode as u32,
                rec_len: 0,
                name_len: 0,
                file_type,
                name: [0 as u8; EXT2_NAME_LEN],
            };
        e.update_name(name);
        e
    }
    pub fn new_file(name: &str, inode: usize) -> Self {
        Self::new(name, inode, EXT2_FT_REG_FILE)
    }
    pub fn new_dir(name: &str, inode: usize) -> Self {
        Self::new(name, inode, EXT2_FT_DIR)
    }
}

/**
 * The new version of the directory entry.  Since EXT2 structures are
 * stored in intel byte order, and the name_len field could never be
 * bigger than 255 chars, it's safe to reclaim the extra byte for the
 * file_type field.
 *
 * This structure is deprecated due to endian issues. Please use struct
 * Ext2DirEntry and accessor functions
 *   ext2fs_dirent_name_len
 *   ext2fs_dirent_set_name_len
 *   ext2fs_dirent_file_type
 *   ext2fs_dirent_set_file_type
 * to get and set name_len and file_type fields.
 */
struct Ext2DirEntry2 {
    ///   Inode number 
    pub inode: u32,
    ///   Directory entry length 
    pub rec_len: u16,
    ///   Name length 
    pub name_len: u8,
    pub file_type: u8,
    ///   File name 
    pub name: [u8; EXT2_NAME_LEN],
}

/**
 * Hashes for ext4_dir_entry for casefolded and ecrypted directories.
 * This is located at the first 4 bit aligned location after the name.
 */

struct Ext2DirEntryHash {
    pub hash: le32,
    pub minor_hash: le32,
}

/**
 * This is a bogus directory entry at the end of each leaf block that
 * records checksums.
 */
struct Ext2DirEntryTail {
    ///   Pretend to be unused 
    pub det_reserved_zero1: u32,
    ///   12 
    pub det_rec_len: u16,
    ///   0xDE00, fake namelen/filetype 
    pub det_reserved_name_len: u16,
    ///   crc32c(uuid+inode+dirent) 
    pub det_checksum: u32,
}

/**
 * Ext2 directory file types.  Only the low 3 bits are used.  The
 * other bits are reserved for now.
 */
pub const EXT2_FT_UNKNOWN: u8 = 0;
pub const EXT2_FT_REG_FILE: u8 = 1;
pub const EXT2_FT_DIR: u8 = 2;
pub const EXT2_FT_CHRDEV: u8 = 3;
pub const EXT2_FT_BLKDEV: u8 = 4;
pub const EXT2_FT_FIFO: u8 = 5;
pub const EXT2_FT_SOCK: u8 = 6;
pub const EXT2_FT_SYMLINK: u8 = 7;

pub const EXT2_FT_MAX: u8 = 8;

/**
 * Annoyingly, e2fsprogs always swab16s Ext2DirEntry.name_len, so we
 * have to build Ext2DirEntryTail with that assumption too.  This
 * constant helps to build the dir_entry_tail to look like it has an
 * "invalid" file type.
 */
pub const EXT2_DIR_NAME_LEN_CSUM: usize = 0xDE00;

/**
 * EXT2_DIR_PAD defines the directory entries boundaries
 *
 * NOTE: It must be a multiple of 4
 */
pub const EXT2_DIR_ENTRY_HEADER_LEN: usize = 8;
pub const EXT2_DIR_ENTRY_HASH_LEN: usize = 8;
pub const EXT2_DIR_PAD: usize = 4;
pub const EXT2_DIR_ROUND: usize = EXT2_DIR_PAD - 1;

/**
 * Constants for ext4's extended time encoding
 */
pub const EXT4_EPOCH_BITS: usize = 2;
pub const EXT4_EPOCH_MASK: usize = (1 << EXT4_EPOCH_BITS) - 1;
pub const EXT4_NSEC_MASK: usize = !(0 as usize) << EXT4_EPOCH_BITS;

/**
 * This structure is used for multiple mount protection. It is written
 * into the block number saved in the s_mmp_block field in the superblock.
 * Programs that check MMP should assume that if SEQ_FSCK (or any unknown
 * code above SEQ_MAX) is present then it is NOT safe to use the filesystem,
 * regardless of how old the timestamp is.
 *
 * The timestamp in the MMP structure will be updated by e2fsck at some
 * arbitrary intervals (start of passes, after every few groups of inodes
 * in pass1 and pass1b).  There is no guarantee that e2fsck is updating
 * the MMP block in a timely manner, and the updates it does are purely
 * for the convenience of the sysadmin and not for automatic validation.
 *
 * Note: Only the mmp_seq value is used to determine whether the MMP block
 *    is being updated.  The mmp_time, mmp_nodename, and mmp_bdevname
 *    fields are only for informational purposes for the administrator,
 *    due to clock skew between nodes and hostname HA service takeover.
 */
///   ASCII for MMP 
pub const EXT4_MMP_MAGIC: usize = 0x004D4D50;
///   mmp_seq value for clean unmount 
pub const EXT4_MMP_SEQ_CLEAN: usize = 0xFF4D4D50;
///   mmp_seq value when being fscked 
pub const EXT4_MMP_SEQ_FSCK: usize = 0xE24D4D50;
///   maximum valid mmp_seq value 
pub const EXT4_MMP_SEQ_MAX: usize = 0xE24D4D4F;

/* Not endian-annotated; it's swapped at read/write time */
struct MmpStruct {
    ///   Magic number for MMP 
    pub mmp_magic: u32,
    ///   Sequence no. updated periodically 
    pub mmp_seq: u32,
    ///   Time last updated (seconds) 
    pub mmp_time: u64,
    ///   Node updating MMP block, no NUL? 
    pub mmp_nodename: [u8; 64],
    ///   Bdev updating MMP block, no NUL? 
    pub mmp_bdevname: [u8; 32],
    ///   Changed mmp_check_interval 
    pub mmp_check_interval: u16,
    pub mmp_pad1: u16,
    pub mmp_pad2: [u32; 226],
    ///   crc32c(uuid+mmp_block) 
    pub mmp_checksum: u32,
}

/**
 * Default interval for MMP update in seconds.
 */
pub const EXT4_MMP_UPDATE_INTERVAL: usize = 5;

/**
 * Maximum interval for MMP update in seconds.
 */
pub const EXT4_MMP_MAX_UPDATE_INTERVAL: usize = 300;

/**
 * Minimum interval for MMP checking in seconds.
 */
pub const EXT4_MMP_MIN_CHECK_INTERVAL: usize = 5;

/**
 * Minimum size of inline data.
 */
pub const EXT4_MIN_INLINE_DATA_SIZE: usize = size_of::<u32>() * EXT2_N_BLOCKS;

/**
 * Size of a parent inode in inline data directory.
 */
///   Reject invalid sequences 
pub const EXT4_INLINE_DATA_DOTDOT_SIZE: usize = 4;

pub const EXT4_ENC_UTF8_12_1: usize = 1;

pub const EXT4_ENC_STRICT_MODE_FL: usize = 1 << 0;


