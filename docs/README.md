 # 操作系统实验5报告 - 文件系统

## Overview

本实验要求实现一个基于 FUSE 框架的，类似 EXT2 的文件系统，实现超级块、数据位图、索引位图等主要结构，并可选实现磁盘缓存、文件系统日志等功能。

在本实验中，本项目完成了一个基于 Rust 语言和 [fuse-rs](https://github.com/chiro2001/fuse-rs) 框架的兼容 Ext2 rev 0.0 大部分功能的用户文件系统。

## 源代码结构

本项目由两个 git 仓库组成，仓库 [rfs](https://github.com/chiro2001/rfs) 是 仓库 [fuse-ext2](https://github.com/chiro2001/fuse-ext2) 的子项目。

项目中有四个 Rust crate：

1. `fs/rfs/rfs_bind`，位于 `fuse-ext2` repo 内
   1. 用于将实验提供的 `ddriver` 静态库和 `rfs` 编译到一起形成一个新的静态库 `rfs_bind_lib`
   2. `ddriver` 静态库将包裹为 Rust `struct DDriver`
   3. 同时需要转换 `fuse-rs` 库和 `fuse` C/C++框架之间的 API 逻辑
   4. 编译生成的静态库将交给原实验框架继续和 `rfs.cpp` 一起编译
2. `disk_driver`，位于 `rfs` repo 内
   1. 用于提供磁盘驱动的抽象接口 `trait DiskDriver`
   2. 同时提供了两个简单的磁盘驱动实现，一个是只在内存实现的 `MemoryDiskDriver`，另一个是读写单个文件的 `FileDiskDriver`
3. `src/macro_tools`，位于 `rfs` repo 内
   1. 用于提供一些宏工具
   2. 独立出来的原因是一些导出的宏不允许自身为静态类 crate
4. `rfs`，即 `rfs` repo 自身
   1. 是文件系统的主要逻辑
   2. 通过 `trait DiskDriver` 实现编译期多态
   3. 其同时可编译成静态库 `rfs_lib` 和可执行文件 `rfs_run`
   4. 编译成可执行文件 `rfs_run` 时不需要依赖原项目框架的 `ddriver`，是可以独立运行的

原框架中的项目链接了静态库 `rfs_bind_lib`，所以 `rfs.cpp` 中不含有文件系统逻辑，仅含有向 Rust 端的函数调用。

## 实验原理

### 虚拟磁盘驱动

实验中使用一个 `ddriver` 静态库提供了虚拟磁盘驱动，提供了一个虚拟的容量为 4MiB 的磁盘，每次按块访问 512 字节。

为了 `rfs` 能够独立工作，项目中使用 `FileDiskDriver` 实现了 `trait DiskDriver`，提供了每次 512 字节按块访问的虚拟磁盘接口。经过测试，从 128 KiB 到 4 GiB 等不同容量下，文件系统都能正常运行。

### Ext2 文件系统

[Ext2 (The Second Extended File System)](https://docs.kernel.org/filesystems/ext2.html)，最初于 1993 年 1 月发布。由 R'emy Card、Theodore Ts'o 和 Stephen Tweedie 编写，它是对扩展文件系统的重大改写。目前它仍然是 Linux 使用的主要文件系统之一。

Ext2 与传统的 Unix 文件系统共享许多属性。它具有块、索引节点和目录的概念。它在访问控制列表 (ACL)、片段、取消删除和压缩的规范中有空间，尽管这些尚未实现（一些作为单独的补丁提供）。还有一个版本控制机制，允许以最大兼容的方式添加新功能（例如日志记录）。

### FUSE 框架

[FUSE (Userspace Filesystem)](https://www.kernel.org/doc/html/latest/filesystems/fuse.html) 架构实现了让用户空间提供文件系统的数据、结构和访问方式，而内核提供文件访问方法，于是我们可以通过提供 FUSE 框架的钩子函数完成我们的文件系统的实现和测试，通过挂载用户文件系统的方法与系统本身的文件系统共存。

## 文件系统设计

为了发挥项目的实用性，同时也方便测试，本项目基于 Ext2 rev 0.0 文件系统，实现了其大部分功能。

### 布局设计

对于 Ext2 文件系统而言，磁盘大小、文件系统块大小、磁盘布局并不是固定的。在项目测试中使用的对应 4 MiB 磁盘大小的文件系统布局：

```
# For 4 MiB fs
| BSIZE = 1024 B |
| Boot(1) | Super(1) | GroupDesc(1) | DATA Map(1) | Inode Map(1) | Inode Table(128) | DATA(*) |
```

1. BSIZE：块大小；本项目中实现的文件系统支持 1 KiB、2 KiB、4 KiB 等块大小，这里使用 1 KiB。

2. Boot 块：为了兼容旧电脑平台，这一个文件系统块是为了 MBR 启动准备的，可以储存磁盘引导记录或者分区引导记录。

3. Super 块：文件系统超级块，储存当前文件系统的布局等信息。

   ![ext2_superblock-c500](README.assets/ext2_superblock.jpg)

4. GroupDesc 块：block group descriptor table，储存当前文件系统中每个 block group 对应的 block group descriptor。在本实现中假定了总共只有一个 block group。

   ![block_group_descriptor-c500](README.assets/15508152464129.jpg)

5. DATA Map 块：储存 block bitmap 的块

6. Inode Map 块：储存 inode bitmap 的块

7. Inode Table 共 128 块：储存 inode 信息的块。每个 inode 大小为 128 字节，每个 1 KiB 块能储存 8 个，总共 1024 个 inode。

8. DATA 块：剩下的块都是储存文件数据和文件夹数据的块

以上是在项目测试中使用的文件布局。项目中暂未支持通过估计磁盘大小来动态调整布局参数，故对其他大小的磁盘可以使用 Linux 工具 `mkfs.ext2` 完成。通过 `mkfs.ext2` 在文件中建立一个参数自动的 Ext2 rev 0.0 文件系统：

```bash
mkfs.ext2 ~/ddriver -t ext2 -r 0
```

经测试其和本项目中的文件系统是兼容的。不过因为其会占用部分保留 inode 和 data block，而且会生成 `lost+found`，故测试时还需要使用程序内的格式化逻辑。

### 缓存设计

```
Cache Blks: 512
   Compiling rfs v0.1.0 (/home/chiro/os/fuse-ext2/fs/rfs/rfs)
    Finished release [optimized] target(s) in 8.25s
     Running `target/release/rfs --format -q -c --cache_size 512 /home/chiro/mnt`
Time: 41083.648681640625ms BW: 190.1608121649437MB/s
Cache Blks: 0
    Finished release [optimized] target(s) in 0.05s
     Running `target/release/rfs --format -q /home/chiro/mnt`
Time: 86539.66689109802ms BW: 90.27652035951665MB/s
```