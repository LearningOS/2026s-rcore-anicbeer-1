//! Synchronization and interior mutability primitives

mod condvar;
mod mutex;
mod semaphore;
mod up;

pub use condvar::Condvar;
pub use mutex::{Mutex, MutexBlocking, MutexSpin};
pub use semaphore::Semaphore;
pub use up::UPSafeCell;

use crate::task::ProcessControlBlock;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Check if the current thread would be deadlocked by waiting for `wait_for`.
/// Uses iterative elimination for OR-model deadlock detection.
pub fn detect_deadlock(
    current_tid: usize,
    process: &Arc<ProcessControlBlock>,
    wait_for: Vec<usize>,
) -> bool {
    let process_inner = process.inner_exclusive_access();

    // wait_sets[tid] = list of tids that tid is waiting for
    let mut wait_sets: BTreeMap<usize, Vec<usize>> = BTreeMap::new();

    // Edges from mutex wait queues
    for mutex_opt in process_inner.mutex_list.iter() {
        if let Some(mutex) = mutex_opt {
            if let Some(holder) = mutex.get_holder_tid() {
                for t in mutex.get_wait_tids() {
                    wait_sets.entry(t).or_insert_with(Vec::new).push(holder);
                }
            }
        }
    }

    // Edges from semaphore wait queues
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

    // Add the current thread's prospective wait edges
    if !wait_for.is_empty() {
        wait_sets.insert(current_tid, wait_for);
    }

    // Iterative elimination:
    // If a thread waits for at least one thread NOT in wait_sets,
    // that thread is not deadlocked.
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
