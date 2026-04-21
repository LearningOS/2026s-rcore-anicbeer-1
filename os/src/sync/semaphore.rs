//! Semaphore

use crate::sync::UPSafeCell;
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc, vec::Vec};

/// semaphore structure
pub struct Semaphore {
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
    /// allocations per thread: (tid, count)
    pub alloc: Vec<(usize, usize)>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                    alloc: Vec::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self) {
        trace!("kernel: Semaphore::up");
        let mut inner = self.inner.exclusive_access();
        let tid = current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid;
        if let Some(pos) = inner.alloc.iter().position(|(t, _)| *t == tid) {
            inner.alloc[pos].1 -= 1;
            if inner.alloc[pos].1 == 0 {
                inner.alloc.remove(pos);
            }
        }
        inner.count += 1;
        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                wakeup_task(task);
            }
        }
    }

    /// down operation of semaphore
    pub fn down(&self) {
        trace!("kernel: Semaphore::down");
        let tid = current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid;
        let blocked = {
            let mut inner = self.inner.exclusive_access();
            inner.count -= 1;
            let need_block = inner.count < 0;
            if need_block {
                inner.wait_queue.push_back(current_task().unwrap());
            }
            need_block
        };
        if blocked {
            block_current_and_run_next();
        }
        let mut inner = self.inner.exclusive_access();
        if let Some(entry) = inner.alloc.iter_mut().find(|(t, _)| *t == tid) {
            entry.1 += 1;
        } else {
            inner.alloc.push((tid, 1));
        }
    }

    /// Get the tids waiting for the semaphore
    pub fn get_wait_tids(&self) -> Vec<usize> {
        self.inner
            .exclusive_access()
            .wait_queue
            .iter()
            .map(|task| task.inner_exclusive_access().res.as_ref().unwrap().tid)
            .collect()
    }

    /// Get the allocations of the semaphore
    pub fn get_alloc(&self) -> Vec<(usize, usize)> {
        self.inner.exclusive_access().alloc.clone()
    }

    /// Get the current count of the semaphore
    pub fn get_count(&self) -> isize {
        self.inner.exclusive_access().count
    }
}
