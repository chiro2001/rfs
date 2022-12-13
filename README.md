# RFS

Rust implemented File System.

Based on [fuse-rs](https://github.com/chiro2001/fuse-rs), implement an EXT2 file system.

*NOT COMPLETED YET.*

[Documents/实验报告](docs/README.md)

## Usage

```shell
git clone https://github.com/chiro2001/rfs
cd rfs
cargo run -- -help
```

```shell
$ cargo run -- --help
   Compiling rfs v0.1.0 (/home/chiro/os/fuse-ext2/fs/rfs/rfs)
    Finished dev [unoptimized + debuginfo] target(s) in 2.26s
     Running `target/debug/rfs --help`
Usage: rfs [OPTIONS] [mountpoint]

Arguments:
  [mountpoint]  Optional mountpoint to mount on [default: tests/mnt]

Options:
  -f, --front                    Keep daemon running in front
      --format                   Format disk
      --mkfs                     Use mkfs.ext2 to format disk
  -c, --cache                    Enable caching
      --cache_size <CACHE_SIZE>  Size of cache in blocks [default: 32]
  -r, --read_only                Mount as read only filesystem
  -v, --verbose                  Print more debug information, or set `RUST_LOG=debug`
  -q, --quiet                    Do not print logs
      --latency                  Enable disk latency
  -d, --device <FILE>            Device path (filesystem storage file) [default: ddriver]
  -s, --size <DISK_SIZE>         Size of disk in MiB [default: 4]
      --unit <UNIT>              IO unit of disk in bytes [default: 512]
  -l, --layout <FILE>            Select layout file for formatting disk [default: none]
  -h, --help                     Print help information
  -V, --version                  Print version information
$ 
```

```shell
$ cargo run -- --mkfs -d disk ~/mnt   
    Finished dev [unoptimized + debuginfo] target(s) in 0.05s
     Running `target/debug/rfs --mkfs -d disk /home/chiro/mnt`
[2022-11-30T12:28:55Z INFO  rfs] Device: disk
[2022-11-30T12:28:55Z INFO  rfs] Daemon running at pid: 81716
[2022-11-30T12:28:55Z INFO  rfs] [try 1/3] Mount to /home/chiro/mnt
[2022-11-30T12:28:55Z INFO  fuse::session] Mounting /home/chiro/mnt
[2022-11-30T12:28:55Z INFO  disk_driver::file] FileDrv open: disk                                                           
[2022-11-30T12:28:55Z INFO  rfs::rfs_lib] disk layout size: 4194304
[2022-11-30T12:28:55Z INFO  rfs::rfs_lib] disk unit size: 512
[2022-11-30T12:28:55Z INFO  rfs::rfs_lib] Disk disk has 8192 IO blocks.
[2022-11-30T12:28:55Z INFO  rfs::rfs_lib] disk info: DiskInfo { stats: DiskStats { write_cnt: 0, read_cnt: 0, seek_cnt: 0 }, consts: DiskConst { read_lat: 2, write_lat: 1, seek_lat: 4, track_num: 0, major_num: 100, layout_size: 4194304, iounit_size: 512 } }
[2022-11-30T12:28:55Z INFO  rfs::rfs_lib] super block size 2 disk block (1024 bytes)
[2022-11-30T12:28:55Z INFO  rfs::rfs_lib] FileSystem found!
[2022-11-30T12:28:55Z INFO  rfs::rfs_lib] fs stats: EXT2 1024 inodes, 1 KiB per block, free inodes 1013, free blocks 3950
[2022-11-30T12:28:55Z INFO  rfs::rfs_lib] fs layout:
| BSIZE = 1024 B |
| Boot(1) | Super(1) | GroupDesc(1) | DATA Map(1) | Inode Map(1) | Inode Table(128) | DATA(*) |
[2022-11-30T12:28:55Z INFO  rfs::rfs_lib] For inode bitmap, see @ 1000
[2022-11-30T12:28:55Z INFO  rfs::rfs_lib] For  data bitmap, see @ c00
$ echo a>~/mnt/aaaa
$ fusermount -u ~/mnt
[2022-11-30T12:29:11Z INFO  fuse::session] Unmounted /home/chiro/mnt
[2022-11-30T12:29:11Z INFO  rfs] All Done.                                                                                  
$ 
```

Compatible with [fuse-ext2](https://github.com/alperakcan/fuse-ext2):

```shell
$ file disk
disk: Linux rev 0.0 ext2 filesystem data, UUID=c30eff85-3ac3-4290-9c25-1a166e101635
$ fuse-ext2 disk ~/mnt -o rw+
$ ls ~/mnt -lahi
总计 18K
       1 drwxr-xr-x  3 root  root  1.0K 11月29日 15:40 .
12058626 drwx------ 96 chiro chiro 4.0K 11月30日 20:36 ..
       3 drwxr-xr-x  0 root  root  1.0K 11月30日 20:25 a
       4 -rw-r--r--  0 root  root     2 11月30日 20:29 aaaa
       2 drwx------  2 root  root   12K 11月29日 15:40 lost+found
$ fusermount -u ~/mnt
$ cargo run -- -d disk ~/mnt 
    Finished dev [unoptimized + debuginfo] target(s) in 0.05s
     Running `target/debug/rfs -d disk /home/chiro/mnt`
[2022-11-30T12:37:35Z INFO  rfs] Device: disk
[2022-11-30T12:37:35Z INFO  rfs] Daemon running at pid: 86525
[2022-11-30T12:37:35Z INFO  rfs] [try 1/3] Mount to /home/chiro/mnt
[2022-11-30T12:37:35Z INFO  fuse::session] Mounting /home/chiro/mnt
[2022-11-30T12:37:35Z INFO  disk_driver::file] FileDrv open: disk                                                           
[2022-11-30T12:37:35Z INFO  rfs::rfs_lib] disk layout size: 4194304
[2022-11-30T12:37:35Z INFO  rfs::rfs_lib] disk unit size: 512
[2022-11-30T12:37:35Z INFO  rfs::rfs_lib] Disk disk has 8192 IO blocks.
[2022-11-30T12:37:35Z INFO  rfs::rfs_lib] disk info: DiskInfo { stats: DiskStats { write_cnt: 0, read_cnt: 0, seek_cnt: 0 }, consts: DiskConst { read_lat: 2, write_lat: 1, seek_lat: 4, track_num: 0, major_num: 100, layout_size: 4194304, iounit_size: 512 } }
[2022-11-30T12:37:35Z INFO  rfs::rfs_lib] super block size 2 disk block (1024 bytes)
[2022-11-30T12:37:35Z INFO  rfs::rfs_lib] FileSystem found!
[2022-11-30T12:37:35Z INFO  rfs::rfs_lib] fs stats: EXT2 1024 inodes, 1 KiB per block, free inodes 1013, free blocks 3950
[2022-11-30T12:37:35Z INFO  rfs::rfs_lib] fs layout:
| BSIZE = 1024 B |
| Boot(1) | Super(1) | GroupDesc(1) | DATA Map(1) | Inode Map(1) | Inode Table(128) | DATA(*) |
[2022-11-30T12:37:35Z INFO  rfs::rfs_lib] For inode bitmap, see @ 1000
[2022-11-30T12:37:35Z INFO  rfs::rfs_lib] For  data bitmap, see @ c00
$ ls ~/mnt -lahi
总计 18K
       1 drwxr-xr-x  3 root  root  1.0K 11月29日 15:40 .
12058626 drwx------ 96 chiro chiro 4.0K 11月30日 20:37 ..
      97 drwxr-xr-x  0 root  root  1.0K 11月30日 20:25 a
     100 -rw-r--r--  0 root  root     2 11月30日 20:29 aaaa
      11 drwx------  2 root  root   12K 11月29日 15:40 lost+found
$ mount
// ...
rfs on /home/chiro/mnt type fuse (rw,nosuid,nodev,relatime,user_id=1000,group_id=1000)
$ fusermount -u ~/mnt
[2022-11-30T12:38:57Z INFO  fuse::session] Unmounted /home/chiro/mnt
[2022-11-30T12:38:57Z INFO  rfs] All Done.    
$ 
```

