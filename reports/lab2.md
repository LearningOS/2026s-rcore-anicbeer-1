# Lab 2: 地址空间

## 实验目的

理解 RISC-V SV39 页式虚拟内存管理机制，在操作系统中实现基于页表的地址空间管理，包括内存映射（mmap/munmap）、带页表翻译的系统调用和堆扩展（sbrk）。

## 实验内容

本章实验需要完成以下功能：

1. `sys_mmap`：将物理页帧映射到指定的虚拟地址范围，支持读/写/执行权限设置。
2. `sys_munmap`：解除指定虚拟地址范围的映射。
3. `sys_get_time`：通过页表翻译安全地将时间写入用户空间地址（支持跨页写入）。
4. `sys_trace`：通过查询当前任务的页表实现内存读写和系统调用计数查询。
5. `sys_sbrk`：修改程序断点（heap 顶），实现堆的扩展与收缩。

## 设计与实现

### 1. SV39 页表与地址空间

rCore 采用 RISC-V SV39 三级页表机制，虚拟地址宽度为 39 位。每个任务拥有独立的地址空间（`MemorySet`），包含独立的页表、代码段、数据段、用户栈、`TrapContext` 页和 `TRAMPOLINE` 跳板页。

### 2. sys_mmap / sys_munmap

`sys_mmap` 的实现流程：

1. 检查起始地址是否页对齐，否则返回 `-1`。
2. 检查 `port` 参数合法性（只能使用低 3 位，且不能为 0），否则返回 `-1`。
3. 将 `port` 转换为 `MapPermission`（R/W/X/U）。
4. 调用 `mmap_current`，在 `MemorySet` 中检查目标虚拟页范围是否与已有 `MapArea` 重叠，若重叠则返回 `-1`。
5. 通过 `insert_framed_area` 为目标范围分配物理页帧并建立页表映射。

`sys_munmap` 的实现流程：

1. 检查起始地址和结束地址是否页对齐。
2. 调用 `munmap_current`，在 `MemorySet` 中查找与目标范围完全匹配的 `MapArea`，找到后调用 `unmap` 解除映射并释放物理页帧。

### 3. sys_get_time 的页表安全写入

用户传入的 `TimeVal` 指针可能跨两个物理页。直接使用裸指针写入可能导致页故障。因此使用 `translated_byte_buffer` 将 `TimeVal` 按页拆分为多个 buffer，分别写入，确保安全：

```rust
let buffers = translated_byte_buffer(token, ts as *const u8, core::mem::size_of::<TimeVal>());
let mut time_bytes = [0u8; 16];
// 填充 sec 和 usec ...
let mut offset = 0;
for buffer in buffers {
    let len = buffer.len();
    buffer.copy_from_slice(&time_bytes[offset..offset + len]);
    offset += len;
}
```

### 4. sys_trace 的页表实现

`sys_trace` 需要直接操作用户虚拟地址，必须通过页表查询物理地址并检查权限：

- **Read (request=0)**：通过 `PageTable::from_token(token)` 获取当前页表，调用 `translate(vpn)` 查询 PTE。若 PTE 有效且可读，计算物理地址后读取字节值。
- **Write (request=1)**：类似读操作，但检查 PTE 的可写权限。
- **Syscall Count (request=2)**：直接调用 `current_syscall_times(id)` 返回计数。

**SV39 地址合法性检查**：

在调试中发现 `trace_read(isize::MAX)` 返回了 `Some(0)` 而非 `None`。原因是 `isize::MAX`（`0x7FFFFFFFFFFFFFFF`）是非法 SV39 地址（bit38=1 但高 25 位未全部置 1），但 `VirtAddr::from()` 仅截取低 39 位，导致映射到 `TRAMPOLINE` 区域。为此增加了 SV39 地址合法性检查函数：

```rust
fn is_valid_sv39_va(addr: usize) -> bool {
    let top = addr >> 39;
    let sign = (addr >> 38) & 1;
    if sign == 0 { top == 0 } else { top == 0x1FFFFFF }
}
```

对于非法地址直接返回 `-1`，避免了页表误查。

### 5. find_pte 的 valid 位修正

在实现过程中发现 `PageTable::find_pte` 在到达叶子节点时无条件返回 `Some(pte)`，即使该 PTE 的 `V` 位为 0（未映射）。这导致 `translate()` 对未映射页面返回虚假结果。修正如下：

```rust
if i == 2 {
    if pte.is_valid() {
        result = Some(pte);
    }
    break;
}
```

确保只有真正有效的叶子 PTE 才会被返回。

### 6. sys_sbrk

通过 `change_program_brk` 修改当前任务的 `program_brk`。当 `size > 0` 时扩展堆并映射新的物理页帧；当 `size < 0` 时收缩堆并解除映射。若收缩后试图访问已释放页面，会触发 `PageFault` 并被内核杀死。

## 测试结果

运行 `make test CHAPTER=4`，全部 16 个测试用例通过。
