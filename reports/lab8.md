# Lab 8 死锁检测实验报告

## 实验目的

在本实验中，我们需要为 rCore 操作系统实现死锁检测机制。具体目标包括：

1. 为 Mutex 和 Semaphore 同步原语实现 OR-模型死锁检测；
2. 支持通过系统调用 `enable_deadlock_detect` 开启/关闭死锁检测；
3. 当检测到死锁时，系统调用返回 `-0xdead` 而不是阻塞；
4. 通过 Chapter 8 的所有测试用例。

## 实验原理

### OR-模型死锁检测

在多线程环境中，死锁发生的必要条件是存在循环等待。本实验采用 **OR-模型** 的死锁检测方法：

- 线程 A 等待资源时，如果该资源的持有者集合为 `{B, C}`，则 A 只需要等待 B 或 C 中的任意一个释放资源即可继续；
- 但在检测死锁时，如果 A 的所有等待目标最终都形成循环依赖，则 A 处于死锁状态。

### Wait-for 图与迭代消除法

我们维护一个 **wait-for 图**，其中节点为线程 ID，边表示等待关系。对于 OR-模型，检测算法采用 **迭代消除法**：

1. 构建 wait-for 图：
   - 对于每个阻塞的 Mutex，等待队列中的每个线程都指向 Mutex 的持有者；
   - 对于每个信号量值为负的 Semaphore，等待队列中的每个线程都指向所有已分配该信号量的线程。
2. 迭代消除不可能死锁的节点：
   - 如果一个线程等待的线程集合中，至少有一个线程 **不在** wait-for 图中，则该线程不可能死锁，将其从图中移除；
   - 重复此过程直到没有节点可被移除。
3. 若当前线程仍在图中，则说明存在死锁环，返回死锁。

## 实现细节

### 1. Mutex 持有者追踪 (`os/src/sync/mutex.rs`)

为 `MutexBlocking` 增加 `holder` 字段，用于记录当前持有 Mutex 的线程 ID：

```rust
pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
    holder: Option<usize>,  // 新增
}
```

在 `lock()` 中设置持有者：

```rust
mutex_inner.locked = true;
mutex_inner.holder = Some(current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid);
```

在 `unlock()` 中转移或清除持有者：

```rust
if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
    mutex_inner.holder = Some(waking_task.inner_exclusive_access().res.as_ref().unwrap().tid);
    wakeup_task(waking_task);
} else {
    mutex_inner.locked = false;
    mutex_inner.holder = None;
}
```

为 `Mutex` trait 增加 `get_holder_tid()` 和 `get_wait_tids()` 方法，以便死锁检测模块读取 Mutex 状态。

### 2. Semaphore 分配追踪 (`os/src/sync/semaphore.rs`)

为 `SemaphoreInner` 增加 `alloc` 字段，记录每个线程已分配的信号量数量：

```rust
pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
    pub alloc: Vec<(usize, usize)>,  // (tid, count)
}
```

在 `up()` 中释放分配：

```rust
if let Some(pos) = inner.alloc.iter().position(|(t, _)| *t == tid) {
    inner.alloc[pos].1 -= 1;
    if inner.alloc[pos].1 == 0 {
        inner.alloc.remove(pos);
    }
}
```

在 `down()` 返回后记录分配：

```rust
let mut inner = self.inner.exclusive_access();
if let Some(entry) = inner.alloc.iter_mut().find(|(t, _)| *t == tid) {
    entry.1 += 1;
} else {
    inner.alloc.push((tid, 1));
}
```

同时增加 `get_wait_tids()`、`get_alloc()`、`get_count()` 辅助方法。

### 3. 死锁检测核心算法 (`os/src/sync/mod.rs`)

```rust
pub fn detect_deadlock(
    current_tid: usize,
    process: &Arc<ProcessControlBlock>,
    wait_for: Vec<usize>,
) -> bool {
    let process_inner = process.inner_exclusive_access();
    let mut wait_sets: BTreeMap<usize, Vec<usize>> = BTreeMap::new();

    // 从 Mutex 等待队列构建边
    for mutex_opt in process_inner.mutex_list.iter() {
        if let Some(mutex) = mutex_opt {
            if let Some(holder) = mutex.get_holder_tid() {
                for t in mutex.get_wait_tids() {
                    wait_sets.entry(t).or_insert_with(Vec::new).push(holder);
                }
            }
        }
    }

    // 从 Semaphore 等待队列构建边
    for sem_opt in process_inner.semaphore_list.iter() {
        if let Some(sem) = sem_opt {
            let waits = sem.get_wait_tids();
            let allocs = sem.get_alloc();
            for t in waits {
                for (holder_tid, _) in &allocs {
                    wait_sets.entry(t).or_insert_with(Vec::new).push(*holder_tid);
                }
            }
        }
    }

    drop(process_inner);

    // 加入当前线程的待等待边
    if !wait_for.is_empty() {
        wait_sets.insert(current_tid, wait_for);
    }

    // 迭代消除
    let mut changed = true;
    while changed {
        changed = false;
        let tids: Vec<usize> = wait_sets.keys().cloned().collect();
        for tid in tids {
            if let Some(wait_list) = wait_sets.get(&tid) {
                if wait_list.iter().any(|w| !wait_sets.contains_key(w)) {
                    wait_sets.remove(&tid);
                    changed = true;
                }
            }
        }
    }

    wait_sets.contains_key(&current_tid)
}
```

### 4. 系统调用修改 (`os/src/syscall/sync.rs`)

- **`sys_enable_deadlock_detect`**：设置 `process_inner.deadlock_detect` 标志；
- **`sys_mutex_lock`**：在 `mutex.lock()` 前检查死锁。若开启检测且 Mutex 已被持有，先检查自死锁（`holder == current_tid`），再调用 `detect_deadlock`；
- **`sys_semaphore_down`**：在 `sem.down()` 前检查。若 `deadlock_detect` 为真且 `sem.get_count() <= 0`，说明会阻塞，此时构建 wait-for 边并调用 `detect_deadlock`；
- **返回 `-0xdead`**：检测到死锁时返回该错误码。

### 5. PCB 扩展 (`os/src/task/process.rs`)

在 `ProcessControlBlockInner` 中增加 `deadlock_detect: bool` 字段，并在 `new()` 和 `fork()` 中初始化为 `false`。

### 6. 修复 `sys_get_time` (`os/src/syscall/process.rs`)

在调试过程中发现用户态 `sleep()` 依赖 `get_time()`，而内核的 `sys_get_time` 未实现（返回 `-1`），导致 `sleep` 无限循环。补充实现：

```rust
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    let us = get_time_us();
    let process = current_task().unwrap().process.upgrade().unwrap();
    let token = process.inner_exclusive_access().get_user_token();
    let ts = translated_refmut(token, ts);
    ts.sec = us / 1_000_000;
    ts.usec = us % 1_000_000;
    0
}
```

## 实验结果

运行 `ch8_usertest` 包含的所有 Chapter 8 测试用例，结果全部通过：

```
Usertests: Test ch8_deadlock_mutex1 in Process 16 exited with code 0
Usertests: Test ch8_deadlock_sem1 in Process 17 exited with code 0
Usertests: Test ch8_deadlock_sem2 in Process 18 exited with code 0
Usertests: Test ch8b_mpsc_sem in Process 19 exited with code 0
Usertests: Test ch8b_phil_din_mutex in Process 20 exited with code 0
Usertests: Test ch8b_race_adder_mutex_spin in Process 21 exited with code 0
Usertests: Test ch8b_sync_sem in Process 22 exited with code 0
Usertests: Test ch8b_test_condvar in Process 23 exited with code 0
Usertests: Test ch8b_threads in Process 24 exited with code 0
Usertests: Test ch8b_threads_arg in Process 25 exited with code 0
ch8 Usertests passed28282!
```

- `ch8_deadlock_mutex1`：检测到同一线程重复获取同一 Mutex 导致的自死锁；
- `ch8_deadlock_sem1`：检测到多个线程循环等待信号量资源导致的死锁；
- `ch8_deadlock_sem2`：资源充足，未检测到死锁，所有线程正常退出。

## 总结

本实验成功实现了基于 OR-模型的死锁检测机制。核心思路是：

1. 为 Mutex 和 Semaphore 增加足够的状态追踪（持有者、等待队列、分配记录）；
2. 在阻塞性系统调用（`mutex_lock`、`semaphore_down`）执行前，构建 wait-for 图并进行迭代消除；
3. 若当前线程处于死锁环中，则提前返回 `-0xdead`，避免真正阻塞。

通过该实现，Chapter 8 所有测试用例全部通过。
