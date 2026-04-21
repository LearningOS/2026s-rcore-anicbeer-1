# Lab 1: 多道程序与分时多任务

## 一、实验目的

1. 理解 RISC-V 特权级切换（U-mode 与 S-mode 之间）的机制，掌握 Trap 处理的完整流程。
2. 理解任务上下文（Task Context）与 Trap 上下文（Trap Context）的区别，掌握任务切换的实现原理。
3. 实现基于时间片轮转的多任务调度，使操作系统支持多个用户程序的分时复用。
4. 实现 `sys_trace` 系统调用，支持对用户态内存的读写以及系统调用次数的统计。

## 二、实验内容

本章在第二章批处理系统的基础上，将单道批处理升级为**多道程序与分时多任务**系统。主要修改和新增内容如下：

- **任务管理**：引入 `TaskManager` 管理多个 `TaskControlBlock`，每个任务拥有独立的内核栈与用户栈。
- **任务切换**：通过 `__switch` 汇编函数保存当前任务的 callee-saved 寄存器，并恢复下一个任务的上下文。
- **Trap 处理**：时钟中断触发任务抢占，实现基于时间片的轮转调度；系统调用通过 `ecall` 陷入内核处理。
- **系统调用**：实现 `sys_yield`、`sys_get_time` 以及实验要求的 `sys_trace`。

## 三、关键设计与实现

### 3.1 任务管理结构

内核使用 `TaskManager` 统一管理所有任务，其核心数据结构如下：

```rust
pub struct TaskManager {
    num_app: usize,
    inner: UPSafeCell<TaskManagerInner>,
}

pub struct TaskManagerInner {
    tasks: [TaskControlBlock; MAX_APP_NUM],
    current_task: usize,
}
```

每个 `TaskControlBlock` 记录任务的状态（Ready / Running / Exited）和任务上下文 `TaskContext`：

```rust
pub struct TaskContext {
    ra: usize,          // 返回地址
    sp: usize,          // 内核栈指针
    s: [usize; 12],     // s0-s11
}
```

初始化时，所有应用被加载到各自的内存区域，每个任务的 `TaskContext` 被设置为指向 `__restore`，从而首次切换时能够正确恢复到用户态执行。

### 3.2 任务切换机制

任务切换由 `__switch` 汇编函数完成（见 `task/switch.S`）。它接收两个参数：
- `a0`：当前任务的 `TaskContext` 指针（保存现场）
- `a1`：下一个任务的 `TaskContext` 指针（恢复现场）

`__switch` 保存当前任务的 `ra`、`sp`、`s0-s11`，然后恢复下一个任务的同名寄存器，最后执行 `ret` 跳转到新任务的 `ra` 处继续执行。

调度策略采用**简单轮转**：当前任务挂起或退出后，从 `current_task + 1` 开始顺序查找下一个 `Ready` 状态的任务。时钟中断（10ms 一次）会强制当前任务挂起，从而实现抢占式调度。

### 3.3 Trap 处理流程

所有 Trap（异常和中断）统一通过 `stvec` 指向的 `__alltraps` 入口处理：

1. **进入 Trap**：`__alltraps` 将用户栈切换到内核栈，保存 `x0-x31`、`sstatus`、`sepc` 到内核栈上的 `TrapContext`，然后调用 Rust 函数 `trap_handler`。
2. **处理 Trap**：
   - `UserEnvCall`（系统调用）：`sepc += 4` 跳过 `ecall` 指令，根据 `a7` 中的 syscall ID 分发处理。
   - `SupervisorTimer`（时钟中断）：设置下一次定时器中断，调用 `suspend_current_and_run_next()` 切换任务。
   - 非法指令 / 页错误：打印错误信息，终止当前任务。
3. **恢复 Trap**：`__restore` 从 `TrapContext` 恢复寄存器，通过 `sret` 返回用户态。

### 3.4 sys_trace 系统调用的实现

实验要求实现 `sys_trace`，支持三种操作：

1. **Read（request=0）**：从用户地址 `id` 处读取一个字节并返回。
2. **Write（request=1）**：向用户地址 `id` 处写入一个字节 `data`。
3. **Syscall（request=2）**：返回当前任务对 syscall `id` 的调用次数。

#### 3.4.1 系统调用计数

首先在 `TaskControlBlock` 中增加 `syscall_times` 字段：

```rust
pub const MAX_SYSCALL_NUM: usize = 512;

pub struct TaskControlBlock {
    pub task_status: TaskStatus,
    pub task_cx: TaskContext,
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
}
```

在 `syscall/mod.rs` 的入口函数中，每次系统调用前递增计数：

```rust
pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    crate::task::increase_syscall_count(syscall_id);
    match syscall_id {
        // ...
    }
}
```

`increase_syscall_count` 通过 `TaskManager` 获取当前任务索引，对 `syscall_times[syscall_id]` 加一。

#### 3.4.2 sys_trace 函数

```rust
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    match trace_request {
        0 => {
            let ptr = id as *const u8;
            unsafe { *ptr as isize }
        }
        1 => {
            let ptr = id as *mut u8;
            unsafe { *ptr = data as u8; }
            0
        }
        2 => {
            crate::task::current_syscall_times(id) as isize
        }
        _ => -1,
    }
}
```

由于当前实现尚未启用页表隔离，内核态可以直接访问用户态地址空间，因此读写操作可以直接通过裸指针完成。`trace_read` 和 `trace_write` 为后续内存隔离后的实现预留了接口语义。

## 四、实验结果

运行 `make test CHAPTER=3`，测试结果如下：

| 测试项目 | 结果 |
|---------|------|
| ch3b_yield0/1/2（多任务 yield 交替输出） | PASS |
| ch3_sleep（忙等 sleep） | PASS |
| ch3_sleep1（定时睡眠） | PASS |
| ch3_trace（trace 读写与 syscall 计数） | PASS |

全部 7 项测试均通过：

```
[PASS] found <get_time OK! (\d+)>
[PASS] found <Test sleep OK!>
[PASS] found <current time_msec = (\d+)>
[PASS] found <time_msec = (\d+) after sleeping (\d+) ticks, delta = (\d+)ms!>
[PASS] found <Test sleep1 passed!>
[PASS] found <string from task trace test>
[PASS] found <Test trace OK!>

Test passed: 7/7
```

运行截图显示三个 yield 测试程序（A/B/C）正确交替输出，power 计算程序在时间片轮转下并行执行，trace 测试正确读取内存、修改内存并统计了 `get_time`、`trace`、`yield`、`write` 等系统调用的次数。

## 五、心得体会

通过本章实验，我深入理解了操作系统多任务调度的核心机制。与第二章的单道批处理相比，多道程序的关键在于：

1. **上下文的保存与恢复**：区分 Task Context（任务切换时保存）和 Trap Context（Trap 处理时保存）非常重要。前者只保存 callee-saved 寄存器（因为编译器保证 caller-saved 已在调用者栈中），后者需要保存完整通用寄存器状态。

2. **时钟中断与抢占**：通过 RISC-V 的 `sie` 和 `stimer` 使能定时器中断，内核能够在用户程序运行途中强制收回控制权，这是实现公平调度的基础。

3. **系统调用统计**：通过在 TCB 中维护计数数组，可以方便地实现对每个任务 syscall 行为的追踪。这一机制在后续的 strace 等调试工具中有直接应用。

`sys_trace` 的实现让我体会到内核态与用户态地址空间的关系。在当前章节尚未引入页表隔离的情况下，内核可以直接解引用用户指针；后续章节引入虚拟内存后，需要通过页表翻译地址，这也是 `sys_trace` 接口设计需要预留扩展性的原因。
