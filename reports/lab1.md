# Lab 1: 多道程序与分时多任务

## 实验目的

理解多道程序设计和分时多任务调度的基本原理，在操作系统中实现任务切换、系统调用追踪和计时功能。

## 实验内容

本章实验需要在 rCore 中完成以下功能：

1. `sys_yield`：当前任务主动放弃 CPU，切换到下一个就绪任务。
2. `sys_get_time`：获取当前时间（秒和微秒），写入用户指定的地址。
3. `sys_trace`：提供系统调用追踪功能，包括：
   - 读取指定用户地址的一个字节
   - 写入指定用户地址的一个字节
   - 查询指定系统调用的调用次数

## 设计与实现

### 1. 系统调用计数

为支持 `sys_trace` 的查询功能，在 `TaskControlBlock` 中增加了 `syscall_times` 数组：

```rust
pub struct TaskControlBlock {
    // ...
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
}
```

在 `syscall/mod.rs` 的 `syscall()` 分发函数入口处，增加对当前任务系统调用次数的统计：

```rust
pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    increase_syscall_count(syscall_id);
    match syscall_id {
        // ...
    }
}
```

### 2. sys_yield 与多任务调度

`sys_yield` 调用 `suspend_current_and_run_next()`，将当前任务状态改为 `Ready`，然后通过 `__switch` 切换到下一个就绪任务。任务调度采用简单的轮转策略：从当前任务向后查找第一个状态为 `Ready` 的任务。

### 3. sys_get_time

使用 `get_time_us()` 获取微秒级时间戳，拆分为秒和微秒，写入用户提供的 `TimeVal` 结构体指针：

```rust
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    let us = get_time_us();
    let sec = us / 1_000_000;
    let usec = us % 1_000_000;
    // 写入用户地址 ...
}
```

### 4. sys_trace

`sys_trace` 支持三种 `trace_request`：

- `0`：从用户地址 `id` 读取一个字节。需要检查地址是否在用户合法范围内。
- `1`：向用户地址 `id` 写入一个字节 `data`。
- `2`：查询系统调用 `id` 的调用次数，返回计数值。

对于非法地址或越界访问，返回 `-1` 或 `None`。

## 测试结果

运行 `make test CHAPTER=3`，所有 7 个测试用例全部通过。
