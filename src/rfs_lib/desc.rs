// see: https://www.nongnu.org/ext2-doc/ext2.html
#![allow(dead_code)]
#![allow(unused_variables)]
/*
 * Define EXT2_PREALLOCATE to preallocate data blocks for expanding files
 */
use std::mem::size_of;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, NaiveDateTime, Utc};
use fuse::{FileAttr, FileType};
use rand::Rng;
use crate::prv;
use crate::rfs_lib::types::{le16, le32, s16};

pub const EXT2_DEFAULT_PREALLOC_BLOCKS: usize = 8;

/*
 * The second extended file system version
 */
pub const EXT2FS_DATE: &'static str = "95/08/09";
pub const EXT2FS_VERSION: &'static str = "0.5b";

/*
 * Special inode numbers
 */
pub const EXT2_BAD_INO: usize = 1         /* Bad blocks inode */;
pub const EXT2_ROOT_INO: usize = 2        /* Root inode */;
pub const EXT4_USR_QUOTA_INO: usize = 3   /* User quota inode */;
pub const EXT4_GRP_QUOTA_INO: usize = 4   /* Group quota inode */;
pub const EXT2_BOOT_LOADER_INO: usize = 5 /* Boot loader inode */;
pub const EXT2_UNDEL_DIR_INO: usize = 6   /* Undelete directory inode */;
pub const EXT2_RESIZE_INO: usize = 7      /* Reserved group descriptors inode */;
pub const EXT2_JOURNAL_INO: usize = 8     /* Journal inode */;
pub const EXT2_EXCLUDE_INO: usize = 9     /* The "exclude" inode, for snapshots */;
pub const EXT4_REPLICA_INO: usize = 10    /* Used by non-upstream feature */;

/* First non-reserved inode for old ext2 filesystems */
pub const EXT2_GOOD_OLD_FIRST_INO: usize = 11;

/*
 * The second extended file system magic number
 */
pub const EXT2_SUPER_MAGIC: u16 = 0xEF53;
/*
 * Maximal count of links to a file
 */
pub const EXT2_LINK_MAX: usize = 65000;

/*
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
    pub acle_perms: u16, /* Access permissions */
    pub acle_type: u16,  /* Type of entry */
    pub acle_tag: u16,   /* User or group identity */
    pub acle_pad1: u16,
    pub acle_next: u32, /* Pointer on next entry for the */
    /* same inode or on next free entry */
}

/*
 * Structure of a blocks group descriptor
 */
#[derive(Debug, Default)]
#[repr(C, align(2))]
pub struct Ext2GroupDesc {
    pub bg_block_bitmap: u32,      /* Blocks bitmap block */
    pub bg_inode_bitmap: u32,      /* Inodes bitmap block */
    pub bg_inode_table: u32,       /* Inodes table block */
    pub bg_free_blocks_count: u16, /* Free blocks count */
    pub bg_free_inodes_count: u16, /* Free inodes count */
    pub bg_used_dirs_count: u16,   /* Directories count */
    pub bg_flags: u16,
    pub bg_exclude_bitmap_lo: u32,    /* Exclude bitmap for snapshots */
    pub bg_block_bitmap_csum_lo: u16, /* crc32c(s_uuid+grp_num+bitmap) LSB */
    pub bg_inode_bitmap_csum_lo: u16, /* crc32c(s_uuid+grp_num+bitmap) LSB */
    pub bg_itable_unused: u16,        /* Unused inodes count */
    pub bg_checksum: u16,             /* crc16(s_uuid+group_num+group_desc)*/
}

pub const EXT2_BG_INODE_UNINIT: usize = 0x0001 /* Inode table/bitmap not initialized */;
pub const EXT2_BG_BLOCK_UNINIT: usize = 0x0002 /* Block bitmap not initialized */;
pub const EXT2_BG_INODE_ZEROED: usize = 0x0004 /* On-disk itable initialized to zero */;

/*
 * Data structures used by the directory indexing feature
 *
 * Note: all of the multibyte integer fields are little endian.
 */

/*
 * Note: dx_root_info is laid out so that if it should somehow get
 * overlaid by a dirent the two low bits of the hash version will be
 * zero.  Therefore, the hash version mod 4 should never be 0.
 * Sincerely, the paranoia department.
 */
// struct ext2_dx_root_info {
//     pub reserved_zero: u32,
//     pub hash_version: u8, /* 0 now, 1 at release */
//     pub info_length: u8,  /* 8 */
//     pub indirect_levels: u8,
//     pub unused_flags: u8,
// };


pub const EXT2_HASH_LEGACY: usize = 0;
pub const EXT2_HASH_HALF_MD4: usize = 1;
pub const EXT2_HASH_TEA: usize = 2;
pub const EXT2_HASH_LEGACY_UNSIGNED: usize = 3   /* reserved for userspace lib */;
pub const EXT2_HASH_HALF_MD4_UNSIGNED: usize = 4 /* reserved for userspace lib */;
pub const EXT2_HASH_TEA_UNSIGNED: usize = 5      /* reserved for userspace lib */;
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

/*
 * Constants relative to the data blocks
 */
pub const EXT2_NDIR_BLOCKS: usize = 12;
pub const EXT2_IND_BLOCK: usize = EXT2_NDIR_BLOCKS;
pub const EXT2_DIND_BLOCK: usize = EXT2_IND_BLOCK + 1;
pub const EXT2_TIND_BLOCK: usize = EXT2_DIND_BLOCK + 1;
pub const EXT2_N_BLOCKS: usize = EXT2_TIND_BLOCK + 1;

/*
 * Inode flags
 */
pub const EXT2_SECRM_FL: usize = 0x00000001     /* Secure deletion */;
pub const EXT2_UNRM_FL: usize = 0x00000002      /* Undelete */;
pub const EXT2_COMPR_FL: usize = 0x00000004     /* Compress file */;
pub const EXT2_SYNC_FL: usize = 0x00000008      /* Synchronous updates */;
pub const EXT2_IMMUTABLE_FL: usize = 0x00000010 /* Immutable file */;
pub const EXT2_APPEND_FL: usize = 0x00000020    /* writes to file may only append */;
pub const EXT2_NODUMP_FL: usize = 0x00000040    /* do not dump file */;
pub const EXT2_NOATIME_FL: usize = 0x00000080   /* do not update atime */;
/* Reserved for compression usage... */
pub const EXT2_DIRTY_FL: usize = 0x00000100;
pub const EXT2_COMPRBLK_FL: usize = 0x00000200 /* One or more compressed clusters */;
pub const EXT2_NOCOMPR_FL: usize = 0x00000400  /* Access raw compressed data */;
/* nb: was previously EXT2_ECOMPR_FL */
pub const EXT4_ENCRYPT_FL: usize = 0x00000800  /* encrypted inode */;
/* End compression flags --- maybe not all used */
pub const EXT2_BTREE_FL: usize = 0x00001000 /* btree format dir */;
pub const EXT2_INDEX_FL: usize = 0x00001000 /* hash-indexed directory */;
pub const EXT2_IMAGIC_FL: usize = 0x00002000;
pub const EXT3_JOURNAL_DATA_FL: usize = 0x00004000 /* file data should be journaled */;
pub const EXT2_NOTAIL_FL: usize = 0x00008000       /* file tail should not be merged */;
pub const EXT2_DIRSYNC_FL: usize = 0x00010000   /* Synchronous directory modifications */;
pub const EXT2_TOPDIR_FL: usize = 0x00020000    /* Top of directory hierarchies*/;
pub const EXT4_HUGE_FILE_FL: usize = 0x00040000 /* Set to each huge file */;
pub const EXT4_EXTENTS_FL: usize = 0x00080000   /* Inode uses extents */;
pub const EXT4_VERITY_FL: usize = 0x00100000    /* Verity protected inode */;
pub const EXT4_EA_INODE_FL: usize = 0x00200000  /* Inode used for large EA */;
/* EXT4_EOFBLOCKS_FL 0x00400000 was here */
pub const FS_NOCOW_FL: usize = 0x00800000              /* Do not cow file */;
pub const EXT4_SNAPFILE_FL: usize = 0x01000000         /* Inode is a snapshot */;
pub const FS_DAX_FL: usize = 0x02000000                /* Inode is DAX */;
pub const EXT4_SNAPFILE_DELETED_FL: usize = 0x04000000 /* Snapshot is being deleted */;
pub const EXT4_SNAPFILE_SHRUNK_FL: usize = 0x08000000  /* Snapshot shrink has completed */;
pub const EXT4_INLINE_DATA_FL: usize = 0x10000000      /* Inode has inline data */;
pub const EXT4_PROJINHERIT_FL: usize = 0x20000000      /* Create with parents projid */;
pub const EXT4_CASEFOLD_FL: usize = 0x40000000         /* Casefolded file */;
pub const EXT2_RESERVED_FL: usize = 0x80000000         /* reserved for ext2 lib */;

pub const EXT2_FL_USER_VISIBLE: usize = 0x604BDFFF    /* User visible flags */;
pub const EXT2_FL_USER_MODIFIABLE: usize = 0x604B80FF /* User modifiable flags */;

#[derive(Debug)]
#[repr(C, align(2))]
pub struct Ext2INode {
    /*00*/ pub i_mode: u16,  /* File mode */
    pub i_uid: u16,          /* Low 16 bits of Owner Uid */
    pub i_size: u32,         /* Size in bytes */
    pub i_atime: u32,        /* Access time */
    pub i_ctime: u32,        /* Inode change time */
    /*10*/ pub i_mtime: u32, /* Modification time */
    pub i_dtime: u32,        /* Deletion Time */
    pub i_gid: u16,          /* Low 16 bits of Group Id */
    pub i_links_count: u16,  /* Links count */
    pub i_blocks: u32,       /* Blocks count */
    /*20*/ pub i_flags: u32, /* File flags */
    pub i_version: u32, /* was l_i_reserved1 */
    /*28*/ pub i_block: [u32; EXT2_N_BLOCKS], /* Pointers to blocks */
    /*64*/ pub i_generation: u32,           /* File version (for NFS) */
    pub i_file_acl: u32,                    /* File ACL */
    pub i_size_high: u32,
    /*70*/ pub i_faddr: u32, /* Fragment address */
    pub i_blocks_hi: u16,
    pub i_file_acl_high: u16,
    pub i_uid_high: u16,    /* these 2 fields    */
    pub i_gid_high: u16,    /* were reserved2[0] */
    pub i_checksum_lo: u16, /* crc32c(uuid+inum+inode) */
    pub i_reserved: u16,
}

pub const EXT2_INODE_SIZE: usize = size_of::<Ext2INode>();

pub fn utc_time(timestamp_seconds: u32) -> SystemTime {
    let naive = NaiveDateTime::from_timestamp_millis(timestamp_seconds as i64 * 1000).unwrap();
    let datetime: DateTime<Utc> = DateTime::from_utc(naive, Utc);
    SystemTime::from(datetime)
}

impl Ext2INode {
    pub fn to_attr(self: &Self, ino: usize) -> FileAttr {
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
            flags: 0
        }
    }
}

impl Default for Ext2INode {
    fn default() -> Self {
        Self {
            i_mode: 0,
            i_uid: 0,
            i_size: 0,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
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
            i_reserved: 0
        }
    }
}

/*
 * File system states
 */
pub const EXT2_VALID_FS: usize = 0x0001  /* Unmounted cleanly */;
pub const EXT2_ERROR_FS: usize = 0x0002  /* Errors detected */;
pub const EXT3_ORPHAN_FS: usize = 0x0004 /* Orphans being recovered */;
pub const EXT4_FC_REPLAY: usize = 0x0020 /* Ext4 fast commit replay ongoing */;

/*
 * Misc. filesystem flags
 */
pub const EXT2_FLAGS_SIGNED_HASH: usize = 0x0001   /* Signed dirhash in use */;
pub const EXT2_FLAGS_UNSIGNED_HASH: usize = 0x0002 /* Unsigned dirhash in use */;
pub const EXT2_FLAGS_TEST_FILESYS: usize = 0x0004  /* OK for use on development code */;
pub const EXT2_FLAGS_IS_SNAPSHOT: usize = 0x0010   /* This is a snapshot image */;
pub const EXT2_FLAGS_FIX_SNAPSHOT: usize = 0x0020  /* Snapshot inodes corrupted */;
pub const EXT2_FLAGS_FIX_EXCLUDE: usize = 0x0040   /* Exclude bitmaps corrupted */;

/*
 * Mount flags
 */
pub const EXT2_MOUNT_CHECK: usize = 0x0001        /* Do mount-time checks */;
pub const EXT2_MOUNT_GRPID: usize = 0x0004        /* Create files with directory's group */;
pub const EXT2_MOUNT_DEBUG: usize = 0x0008        /* Some debugging messages */;
pub const EXT2_MOUNT_ERRORS_CONT: usize = 0x0010  /* Continue on errors */;
pub const EXT2_MOUNT_ERRORS_RO: usize = 0x0020    /* Remount fs ro on errors */;
pub const EXT2_MOUNT_ERRORS_PANIC: usize = 0x0040 /* Panic on errors */;
pub const EXT2_MOUNT_MINIX_DF: usize = 0x0080     /* Mimics the Minix statfs */;
pub const EXT2_MOUNT_NO_UID32: usize = 0x0200     /* Disable 32-bit UIDs */;

/*
 * Maximal mount counts between two filesystem checks
 */
pub const EXT2_DFL_MAX_MNT_COUNT: usize = 20 /* Allow 20 mounts */;
pub const EXT2_DFL_CHECKINTERVAL: usize = 0  /* Don't use interval check */;

/*
 * Behaviour when detecting errors
 */
pub const EXT2_ERRORS_CONTINUE: usize = 1 /* Continue execution */;
pub const EXT2_ERRORS_RO: usize = 2       /* Remount fs read-only */;
pub const EXT2_ERRORS_PANIC: usize = 3    /* Panic */;
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

/*
 * Structure of the super block
 */
#[derive(Debug)]
#[repr(C, align(2))]
pub struct Ext2SuperBlock {
    /*000*/ pub s_inodes_count: u32,      /* Inodes count */
    pub s_blocks_count: u32,              /* Blocks count */
    pub s_r_blocks_count: u32,            /* Reserved blocks count */
    pub s_free_blocks_count: u32,         /* Free blocks count */
    /*010*/ pub s_free_inodes_count: u32, /* Free inodes count */
    pub s_first_data_block: u32,          /* First Data Block */
    pub s_log_block_size: u32,            /* Block size */
    pub s_log_cluster_size: u32,          /* Allocation cluster size */
    /*020*/ pub s_blocks_per_group: u32,  /* # Blocks per group */
    pub s_clusters_per_group: u32,        /* # Fragments per group */
    pub s_inodes_per_group: u32,          /* # Inodes per group */
    pub s_mtime: u32,                     /* Mount time */
    /*030*/ pub s_wtime: u32,             /* Write time */
    pub s_mnt_count: u16,                 /* Mount count */
    pub s_max_mnt_count: s16,             /* Maximal mount count */
    pub s_magic: u16,                     /* Magic signature */
    pub s_state: u16,                     /* File system state */
    pub s_errors: u16,                    /* Behaviour when detecting errors */
    pub s_minor_rev_level: u16,           /* minor revision level */
    /*040*/ pub s_lastcheck: u32,         /* time of last check */
    pub s_checkinterval: u32,             /* max. time between checks */
    pub s_creator_os: u32,                /* OS */
    pub s_rev_level: u32,                 /* Revision level */
    /*050*/ pub s_def_resuid: u16,        /* Default uid for reserved blocks */
    pub s_def_resgid: u16,                /* Default gid for reserved blocks */
    /*
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
    pub s_first_ino: u32,                   /* First non-reserved inode */
    pub s_inode_size: u16,                  /* size of inode structure */
    pub s_block_group_nr: u16,              /* block group # of this superblock */
    pub s_feature_compat: u32,              /* compatible feature set */
    /*060*/ pub s_feature_incompat: u32,    /* incompatible feature set */
    pub s_feature_ro_compat: u32,           /* readonly-compatible feature set */
    /*068*/ pub s_uuid: [u8; 16], /* 128-bit uuid for volume */
    /*078*/ pub s_volume_name: [u8; EXT2_LABEL_LEN], /* volume name, no NUL? */
    /*088*/ pub s_last_mounted: [u8; 64], /* directory last mounted on, no NUL? */
    /*0c8*/ pub s_algorithm_usage_bitmap: u32, /* For compression */
    /*
     * Performance hints.  Directory preallocation should only
     * happen if the EXT2_FEATURE_COMPAT_DIR_PREALLOC flag is on.
     */
    pub s_prealloc_blocks: u8,      /* Nr of blocks to try to preallocate*/
    pub s_prealloc_dir_blocks: u8,  /* Nr to preallocate for dirs */
    pub s_reserved_gdt_blocks: u16, /* Per group table for online growth */
    /*
     * Journaling support valid if EXT2_FEATURE_COMPAT_HAS_JOURNAL set.
     */
    /*0d0*/ pub s_journal_uuid: [u8; 16], /* uuid of journal superblock */
    /*0e0*/ pub s_journal_inum: u32,       /* inode number of journal file */
    pub s_journal_dev: u32,                /* device number of journal file */
    pub s_last_orphan: u32,                /* start of list of inodes to delete */
    /*0ec*/ pub s_hash_seed: [u32; 4],       /* HTREE hash seed */
    /*0fc*/ pub s_def_hash_version: u8,    /* Default hash version to use */
    pub s_jnl_backup_type: u8,             /* Default type of journal backup */
    pub s_desc_size: u16,                  /* Group desc. size: INCOMPAT_64BIT */
    /**
     * Other options
     */
    /*100*/ pub s_default_mount_opts: u32, /* default EXT2_MOUNT_* flags used */
    pub s_first_meta_bg: u32,              /* First metablock group */
    pub s_mkfs_time: u32,                  /* When the filesystem was created */
    /*10c*/ pub s_jnl_blocks: [u32; 17],     /* Backup of the journal inode */
    /*150*/ pub s_blocks_count_hi: u32,    /* Blocks count high 32bits */
    pub s_r_blocks_count_hi: u32,          /* Reserved blocks count high 32 bits*/
    pub s_free_blocks_hi: u32,             /* Free blocks count */
    pub s_min_extra_isize: u16,            /* All inodes have at least # bytes */
    pub s_want_extra_isize: u16,           /* New inodes should reserve # bytes */
    /*160*/ pub s_flags: u32,              /* Miscellaneous flags */
    pub s_raid_stride: u16,                /* RAID stride in blocks */
    pub s_mmp_update_interval: u16,        /* # seconds to wait in MMP checking */
    pub s_mmp_block: u64,                  /* Block for multi-mount protection */
    /*170*/ pub s_raid_stripe_width: u32,  /* blocks on all data disks (N*stride)*/
    pub s_log_groups_per_flex: u8,         /* FLEX_BG group size */
    pub s_checksum_type: u8,               /* metadata checksum algorithm */
    pub s_encryption_level: u8,            /* versioning level for encryption */
    pub s_reserved_pad: u8,                /* Padding to next 32bits */
    pub s_kbytes_written: u64,             /* nr of lifetime kilobytes written */
    /*180*/ pub s_snapshot_inum: u32,      /* Inode number of active snapshot */
    pub s_snapshot_id: u32,                /* sequential ID of active snapshot */
    pub s_snapshot_r_blocks_count: u64,    /* active snapshot reserved blocks */
    /*190*/ pub s_snapshot_list: u32,      /* inode number of disk snapshot list */
    // pub const EXT4_S_ERR_START: usize = ext4_offsetof;(struct Ext2SuperBlock, s_error_count)
    pub s_error_count: u32,                     /* number of fs errors */
    pub s_first_error_time: u32,                /* first time an error happened */
    pub s_first_error_ino: u32,                 /* inode involved in first error */
    /*1a0*/ pub s_first_error_block: u64,       /* block involved in first error */
    pub s_first_error_func: [u8; 32], /* function where error hit, no NUL?*/
    /*1c8*/ pub s_first_error_line: u32, /* line number where error happened */
    pub s_last_error_time: u32,          /* most recent time of an error */
    /*1d0*/ pub s_last_error_ino: u32,   /* inode involved in last error */
    pub s_last_error_line: u32,          /* line number where error happened */
    pub s_last_error_block: u64,         /* block involved of last error */
    /*1e0*/ pub s_last_error_func: [u8; 32], /* function where error hit, no NUL? */
    // pub const EXT4_S_ERR_END: usize = ext4_offsetof;(struct Ext2SuperBlock, s_mount_opts)
    /*200*/ pub s_mount_opts: [u8; 64],   /* default mount options, no NUL? */
    /*240*/ pub s_usr_quota_inum: u32,     /* inode number of user quota file */
    pub s_grp_quota_inum: u32,             /* inode number of group quota file */
    pub s_overhead_clusters: u32,          /* overhead blocks/clusters in fs */
    /*24c*/ pub s_backup_bgs: [u32; 2],      /* If sparse_super2 enabled */
    /*254*/ pub s_encrypt_algos: [u8; 4],    /* Encryption algorithms in use  */
    /*258*/ pub s_encrypt_pw_salt: [u8; 16], /* Salt used for string2key algorithm */
    /*268*/ pub s_lpf_ino: le32,           /* Location of the lost+found inode */
    pub s_prj_quota_inum: le32,            /* inode for tracking project quota */
    /*270*/ pub s_checksum_seed: le32,     /* crc32c(orig_uuid) if csum_seed set */
    /*274*/ pub s_wtime_hi: u8,
    pub s_mtime_hi: u8,
    pub s_mkfs_time_hi: u8,
    pub s_lastcheck_hi: u8,
    pub s_first_error_time_hi: u8,
    pub s_last_error_time_hi: u8,
    pub s_first_error_errcode: u8,
    pub s_last_error_errcode: u8,
    /*27c*/ pub s_encoding: le16, /* Filename charset encoding */
    pub s_encoding_flags: le16,   /* Filename charset encoding flags */
    pub s_reserved: [le32; 95],     /* Padding to the end of the block */
    /*3fc*/ pub s_checksum: u32,  /* crc32c(superblock) */
}

pub fn create_uuid() -> [u8; 16] {
    let mut rng = rand::thread_rng();
    (0..16).map(|_| { rng.gen::<u8>() }).collect::<Vec<u8>>().try_into().unwrap()
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
            s_wtime: 1669521656,
            s_mnt_count: 0,
            s_max_mnt_count: 65535,
            s_magic: 61267,
            s_state: 1,
            s_errors: 1,
            s_minor_rev_level: 0,
            s_lastcheck: 1669521656,
            s_checkinterval: 0,
            s_creator_os: 0,
            s_rev_level: 1,
            s_def_resuid: 0,
            s_def_resgid: 0,
            s_first_ino: 11,
            s_inode_size: 256,
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
            s_mkfs_time: 1669521656,
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
    pub fn magic_matched(self: &Self) -> bool { self.s_magic == EXT2_SUPER_MAGIC }
    pub fn block_size(self: &Self) -> usize { 1 << self.s_log_block_size }
}

/*
 * Codes for operating systems
 */
pub const EXT2_OS_LINUX: usize = 0;
pub const EXT2_OS_HURD: usize = 1;
pub const EXT2_OBSO_OS_MASIX: usize = 2;
pub const EXT2_OS_FREEBSD: usize = 3;
pub const EXT2_OS_LITES: usize = 4;

/*
 * Revision levels
 */
pub const EXT2_GOOD_OLD_REV: usize = 0 /* The good old (original) format */;
pub const EXT2_DYNAMIC_REV: usize = 1  /* V2 format w/ dynamic inode sizes */;

pub const EXT2_CURRENT_REV: usize = EXT2_GOOD_OLD_REV;
pub const EXT2_MAX_SUPP_REV: usize = EXT2_DYNAMIC_REV;

pub const EXT2_GOOD_OLD_INODE_SIZE: usize = 128;

/*
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
/*
 * METADATA_CSUM implies GDT_CSUM.  When METADATA_CSUM is set, group
 * descriptor checksums use the same algorithm as all other data
 * structures' checksums.
 */
pub const EXT4_FEATURE_RO_COMPAT_METADATA_CSUM: usize = 0x0400;
pub const EXT4_FEATURE_RO_COMPAT_REPLICA: usize = 0x0800;
pub const EXT4_FEATURE_RO_COMPAT_READONLY: usize = 0x1000;
pub const EXT4_FEATURE_RO_COMPAT_PROJECT: usize = 0x2000 /* Project quota */;
pub const EXT4_FEATURE_RO_COMPAT_SHARED_BLOCKS: usize = 0x4000;
pub const EXT4_FEATURE_RO_COMPAT_VERITY: usize = 0x8000;

pub const EXT2_FEATURE_INCOMPAT_COMPRESSION: usize = 0x0001;
pub const EXT2_FEATURE_INCOMPAT_FILETYPE: usize = 0x0002;
pub const EXT3_FEATURE_INCOMPAT_RECOVER: usize = 0x0004     /* Needs recovery */;
pub const EXT3_FEATURE_INCOMPAT_JOURNAL_DEV: usize = 0x0008 /* Journal device */;
pub const EXT2_FEATURE_INCOMPAT_META_BG: usize = 0x0010;
pub const EXT3_FEATURE_INCOMPAT_EXTENTS: usize = 0x0040;
pub const EXT4_FEATURE_INCOMPAT_64BIT: usize = 0x0080;
pub const EXT4_FEATURE_INCOMPAT_MMP: usize = 0x0100;
pub const EXT4_FEATURE_INCOMPAT_FLEX_BG: usize = 0x0200;
pub const EXT4_FEATURE_INCOMPAT_EA_INODE: usize = 0x0400;
pub const EXT4_FEATURE_INCOMPAT_DIRDATA: usize = 0x1000;
pub const EXT4_FEATURE_INCOMPAT_CSUM_SEED: usize = 0x2000;
pub const EXT4_FEATURE_INCOMPAT_LARGEDIR: usize = 0x4000    /* >2GB or 3-lvl htree */;
pub const EXT4_FEATURE_INCOMPAT_INLINE_DATA: usize = 0x8000 /* data in inode */;
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

/*
 * Structure of a directory entry
 */
pub const EXT2_NAME_LEN: usize = 255;

#[derive(Debug)]
#[repr(C, align(1))]
pub struct Ext2DirEntry {
    pub inode: u32,              /* Inode number */
    pub rec_len: u16,            /* Directory entry length */
    // pub name_len: u16,           /* Name length */
    // temporally use deprecated structure, for having no logic from cpp
    pub name_len: u8,               /* Name length */
    pub file_type: u8,
    pub name: [u8; EXT2_NAME_LEN], /* File name */
}

pub const EXT2_DIR_ENTRY_BASE_SIZE: usize = size_of::<Ext2DirEntry>() - EXT2_NAME_LEN;

impl Ext2DirEntry {
    pub fn get_name(self: &Self) -> String {
        String::from_utf8_lossy(&self.name[..self.name_len as usize]).to_string()
    }
    pub fn to_string(self: &Self) -> String {
        format!("{} {} entry size {} name size {}", self.inode,
                self.get_name(), self.rec_len, self.name_len)
    }
}

/*
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
    pub inode: u32,   /* Inode number */
    pub rec_len: u16, /* Directory entry length */
    pub name_len: u8, /* Name length */
    pub file_type: u8,
    pub name: [u8; EXT2_NAME_LEN], /* File name */
}

/*
 * Hashes for ext4_dir_entry for casefolded and ecrypted directories.
 * This is located at the first 4 bit aligned location after the name.
 */

struct Ext2DirEntryHash {
    pub hash: le32,
    pub minor_hash: le32,
}

/*
 * This is a bogus directory entry at the end of each leaf block that
 * records checksums.
 */
struct Ext2DirEntryTail {
    pub det_reserved_zero1: u32,    /* Pretend to be unused */
    pub det_rec_len: u16,           /* 12 */
    pub det_reserved_name_len: u16, /* 0xDE00, fake namelen/filetype */
    pub det_checksum: u32,          /* crc32c(uuid+inode+dirent) */
}

/*
 * Ext2 directory file types.  Only the low 3 bits are used.  The
 * other bits are reserved for now.
 */
pub const EXT2_FT_UNKNOWN: usize = 0;
pub const EXT2_FT_REG_FILE: usize = 1;
pub const EXT2_FT_DIR: usize = 2;
pub const EXT2_FT_CHRDEV: usize = 3;
pub const EXT2_FT_BLKDEV: usize = 4;
pub const EXT2_FT_FIFO: usize = 5;
pub const EXT2_FT_SOCK: usize = 6;
pub const EXT2_FT_SYMLINK: usize = 7;

pub const EXT2_FT_MAX: usize = 8;

/*
 * Annoyingly, e2fsprogs always swab16s Ext2DirEntry.name_len, so we
 * have to build Ext2DirEntryTail with that assumption too.  This
 * constant helps to build the dir_entry_tail to look like it has an
 * "invalid" file type.
 */
pub const EXT2_DIR_NAME_LEN_CSUM: usize = 0xDE00;

/*
 * EXT2_DIR_PAD defines the directory entries boundaries
 *
 * NOTE: It must be a multiple of 4
 */
pub const EXT2_DIR_ENTRY_HEADER_LEN: usize = 8;
pub const EXT2_DIR_ENTRY_HASH_LEN: usize = 8;
pub const EXT2_DIR_PAD: usize = 4;
pub const EXT2_DIR_ROUND: usize = EXT2_DIR_PAD - 1;

/*
 * Constants for ext4's extended time encoding
 */
pub const EXT4_EPOCH_BITS: usize = 2;
pub const EXT4_EPOCH_MASK: usize = (1 << EXT4_EPOCH_BITS) - 1;
pub const EXT4_NSEC_MASK: usize = !(0 as usize) << EXT4_EPOCH_BITS;

/*
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
 *	is being updated.  The mmp_time, mmp_nodename, and mmp_bdevname
 *	fields are only for informational purposes for the administrator,
 *	due to clock skew between nodes and hostname HA service takeover.
 */
pub const EXT4_MMP_MAGIC: usize = 0x004D4D50     /* ASCII for MMP */;
pub const EXT4_MMP_SEQ_CLEAN: usize = 0xFF4D4D50 /* mmp_seq value for clean unmount */;
pub const EXT4_MMP_SEQ_FSCK: usize = 0xE24D4D50  /* mmp_seq value when being fscked */;
pub const EXT4_MMP_SEQ_MAX: usize = 0xE24D4D4F   /* maximum valid mmp_seq value */;

/* Not endian-annotated; it's swapped at read/write time */
struct MmpStruct {
    pub mmp_magic: u32,                   /* Magic number for MMP */
    pub mmp_seq: u32,                     /* Sequence no. updated periodically */
    pub mmp_time: u64,                    /* Time last updated (seconds) */
    pub mmp_nodename: [u8; 64], /* Node updating MMP block, no NUL? */
    pub mmp_bdevname: [u8; 32], /* Bdev updating MMP block, no NUL? */
    pub mmp_check_interval: u16,          /* Changed mmp_check_interval */
    pub mmp_pad1: u16,
    pub mmp_pad2: [u32; 226],
    pub mmp_checksum: u32, /* crc32c(uuid+mmp_block) */
}

/*
 * Default interval for MMP update in seconds.
 */
pub const EXT4_MMP_UPDATE_INTERVAL: usize = 5;

/*
 * Maximum interval for MMP update in seconds.
 */
pub const EXT4_MMP_MAX_UPDATE_INTERVAL: usize = 300;

/*
 * Minimum interval for MMP checking in seconds.
 */
pub const EXT4_MMP_MIN_CHECK_INTERVAL: usize = 5;

/*
 * Minimum size of inline data.
 */
pub const EXT4_MIN_INLINE_DATA_SIZE: usize = size_of::<u32>() * EXT2_N_BLOCKS;

/*
 * Size of a parent inode in inline data directory.
 */
pub const EXT4_INLINE_DATA_DOTDOT_SIZE: usize = 4;

pub const EXT4_ENC_UTF8_12_1: usize = 1;

pub const EXT4_ENC_STRICT_MODE_FL: usize = 1 << 0 /* Reject invalid sequences */;


