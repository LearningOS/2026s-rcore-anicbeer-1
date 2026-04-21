# Lab 4: 文件系统

## 实验目的

理解操作系统中文件系统的基本原理，实现文件的创建、读写、元数据查询（fstat）以及硬链接（link/unlink）功能。

## 实验内容

本章实验需要完成以下功能：

1. `sys_fstat`：获取已打开文件的元数据（设备号、inode 号、文件类型、硬链接数）。
2. `sys_linkat`：创建硬链接，使多个文件名指向同一个 inode。
3. `sys_unlinkat`：删除硬链接，当链接数降为 0 时文件不再可通过文件名访问。

此外，ch6 测试还继承了前序章节的功能测试（spawn、mmap、munmap、get_time 等）。

## 设计与实现

### 1. easy-fs 扩展：支持链接计数

easy-fs 原有的 `DiskInode` 结构没有硬链接计数字段。为此在 `DiskInode` 末尾增加了 `nlink: u32` 字段：

```rust
#[repr(C)]
pub struct DiskInode {
    pub size: u32,
    pub direct: [u32; INODE_DIRECT_COUNT],
    pub indirect1: u32,
    pub indirect2: u32,
    type_: DiskInodeType,
    pub nlink: u32,
}
```

在 `initialize` 中将 `nlink` 初始化为 1（创建文件时默认有一个链接）。

### 2. VFS 层 link / unlink

在 `easy-fs/src/vfs.rs` 中为 `Inode` 新增了三个方法：

- **`get_stat`**：读取磁盘 inode 的 `nlink` 和 `type_`，返回 `(dev, ino, mode, nlink)`。其中 `ino` 用 `block_id` 和 `block_offset` 组合而成，保证唯一性。

- **`link(old_name, new_name)`**：
  1. 在当前目录中查找 `old_name`，获取其 `inode_id`。
  2. 检查 `new_name` 是否已存在，若存在则返回 `-1`。
  3. 通过 `modify_disk_inode` 增加目标 inode 的 `nlink`。
  4. 在目录末尾追加一个新的 `DirEntry`，`inode_id` 指向同一个 inode。
  5. 同步块缓存，返回 0。

- **`unlink(name)`**：
  1. 在当前目录中查找 `name`，获取其 `inode_id`。
  2. 减少目标 inode 的 `nlink`（避免下溢）。
  3. 从目录中删除对应的 `DirEntry`：将被删除项与目录末尾项交换，然后减小目录的 `size`。
  4. 同步块缓存，返回 0。

### 3. 内核层系统调用

#### sys_fstat

通过 `fd_table` 获取文件对象。由于 `fd_table` 存储的是 `Arc<dyn File + Send + Sync>`，无法直接 downcast，因此扩展了 `File` trait，增加了 `get_fstat` 方法：

```rust
pub trait File: Send + Sync {
    // ... read/write/readable/writable ...
    fn get_fstat(&self) -> Option<(u64, u64, u32, u32)> {
        None
    }
}
```

`OSInode` 实现了 `get_fstat`，调用底层 `Inode::get_stat()` 返回元数据；`Stdin`/`Stdout` 使用默认实现返回 `None`。

`sys_fstat` 的流程：
1. 检查 fd 合法性。
2. 调用 `file.get_fstat()`，若为 `Some` 则通过 `translated_refmut` 写入用户空间的 `Stat` 结构体。

#### sys_linkat / sys_unlinkat

这两个系统调用直接转发给 `ROOT_INODE.link()` 和 `ROOT_INODE.unlink()`，参数通过 `translated_str` 从用户空间读取。

### 4. 前序功能复用

ch6 分支的 os 代码是全新的基础，因此需要重新实现前序章节的部分功能：

- **`sys_get_time`**：使用 `translated_byte_buffer` 安全写入 `TimeVal`（处理跨页情况）。
- **`sys_mmap` / `sys_munmap`**：复用 ch4 的实现，在 `MemorySet` 中补充 `check_overlap` 和 `remove_area`，在 `task/mod.rs` 中补充 `mmap_current` 和 `munmap_current`。
- **`sys_spawn`**：直接加载 ELF 创建新进程（不复制父进程地址空间），并将新进程加入父进程的 `children` 列表。
- **`find_pte` valid 位修正**：与 ch4 一致，确保只有 `pte.is_valid()` 的叶子节点才会被返回。

### 5. 磁盘布局兼容性

由于修改了 `DiskInode` 的内存布局（增加了 `nlink` 字段），需要重新构建磁盘镜像。测试脚本 `make test` 会在构建用户程序后通过 `easy-fs-fuse` 重新生成镜像，因此不会遇到兼容性问题。

## 测试结果

运行 `make test CHAPTER=6`，全部 31 个测试用例通过。
