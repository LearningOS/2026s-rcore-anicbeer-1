# Lab 3: 进程管理

## 实验目的

理解操作系统中进程的概念，实现进程创建（fork/spawn）、进程替换（exec）、进程同步（waitpid）以及基于 Stride 的比例份额调度算法。

## 实验内容

本章实验需要完成以下功能：

1. `sys_spawn`：直接从一个可执行文件创建新进程，不复制父进程地址空间。
2. `sys_set_priority`：设置进程优先级，支持 Stride 调度器。
3. Stride 调度器：替换 FIFO 调度器，实现比例份额调度。
4. `sys_get_time` / `sys_mmap` / `sys_munmap`：从第四章复用并适配新的进程管理框架。

## 设计与实现

### 1. 进程控制块扩展

在 `TaskControlBlockInner` 中增加了 Stride 调度所需的两个字段：

```rust
pub struct TaskControlBlockInner {
    // ... 已有字段 ...
    /// Priority for stride scheduler
    pub priority: usize,
    /// Pass value for stride scheduler
    pub pass: usize,
}
```

- `priority` 默认值为 16，表示进程的优先级（值越大获得 CPU 份额越多）。
- `pass` 默认值为 0，表示进程的累计调度步长。

在 `fork` 时，子进程继承父进程的 `priority` 和 `pass`，保证父子进程的公平性。

### 2. Stride 调度器

将 `TaskManager` 从简单的 FIFO 队列改造为 Stride 调度器：

- **Stride 计算**：`stride = BIG_STRIDE / priority`，其中 `BIG_STRIDE = 0x7FFFFFFF`。
- **任务选择**：每次 `fetch` 时遍历就绪队列，选择 `pass` 值最小的进程。
- **Pass 更新**：被选中的进程在出队时，`pass += stride`。
- **新任务入队**：新创建或从阻塞态恢复的任务直接加入队列，保持其当前 `pass` 值。

```rust
pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
    let mut min_idx = 0;
    let mut min_pass = usize::MAX;
    for (i, task) in self.ready_queue.iter().enumerate() {
        let pass = task.inner_exclusive_access().pass;
        if pass < min_pass {
            min_pass = pass;
            min_idx = i;
        }
    }
    if min_pass != usize::MAX {
        let task = self.ready_queue.remove(min_idx).unwrap();
        let mut inner = task.inner_exclusive_access();
        inner.pass += BIG_STRIDE / inner.priority;
        drop(inner);
        Some(task)
    } else {
        None
    }
}
```

### 3. sys_spawn

`spawn` 是直接加载 ELF 文件创建新进程的系统调用，与 `fork` 的区别在于：

- `fork` 复制父进程的完整地址空间（通过 `MemorySet::from_existed_user`）。
- `spawn` 直接调用 `TaskControlBlock::new(elf_data)` 创建新地址空间，不复制父进程内存。

实现流程：
1. 通过 `translated_str` 读取用户态传入的文件路径。
2. 调用 `get_app_data_by_name` 获取 ELF 数据。
3. 创建新的 `TaskControlBlock`。
4. 设置新进程的 `parent` 为当前进程。
5. 将新进程添加到当前进程的 `children` 列表中。
6. 将新进程加入就绪队列。
7. 返回新进程的 PID。

### 4. sys_set_priority

设置当前进程的优先级：

- 合法性检查：`prio` 必须 `>= 2`，否则返回 `-1`。
- 通过 `isize::MAX` 的大优先级测试（此时 `stride = 0`，pass 不再增长）。
- 设置成功后返回设置的优先级值。

### 5. mmap / munmap / get_time 的复用

从第四章复用了 `sys_mmap`、`sys_munmap` 和 `sys_get_time` 的实现，并在 `task/mod.rs` 中补充了 `mmap_current` 和 `munmap_current` 接口：

```rust
pub fn mmap_current(start: VirtAddr, end: VirtAddr, perm: MapPermission) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    let memory_set = &mut inner.memory_set;
    if memory_set.check_overlap(start.floor(), end.ceil()) {
        return -1;
    }
    memory_set.insert_framed_area(start, end, perm);
    0
}
```

同时在 `MemorySet` 中补充了 `check_overlap` 和 `remove_area` 方法。

### 6. find_pte 的 valid 位修正

与第四章一致，修复了 `PageTable::find_pte` 在到达叶子节点时无条件返回 `Some(pte)` 的问题，增加了 `pte.is_valid()` 检查：

```rust
if i == 2 {
    if pte.is_valid() {
        result = Some(pte);
    }
    break;
}
```

确保只有真正有效的叶子 PTE 才会被返回。

## 测试结果

运行 `make test CHAPTER=5`，全部 15 个测试用例通过。

Stride 调度测试结果示例：

| Priority | Count | Ratio (Count/Priority) |
|----------|-------|------------------------|
| 5        | 12135600 | 2427120 |
| 6        | 14530800 | 2421800 |
| 7        | 16947200 | 2421028 |
| 8        | 19528400 | 2441050 |
| 9        | 22055600 | 2450622 |
| 10       | 25021600 | 2502160 |

Ratio 值基本接近，说明 Stride 调度器能够按优先级比例分配 CPU 时间。
