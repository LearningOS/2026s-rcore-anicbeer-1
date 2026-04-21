//! Process management syscalls
use crate::mm::{translated_byte_buffer, PageTable, VirtAddr};
use crate::task::{
    change_program_brk, current_syscall_times, current_user_token, exit_current_and_run_next,
    mmap_current, munmap_current, suspend_current_and_run_next,
};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let sec = us / 1_000_000;
    let usec = us % 1_000_000;
    let token = current_user_token();
    let buffers = translated_byte_buffer(token, ts as *const u8, core::mem::size_of::<TimeVal>());
    let mut time_bytes = [0u8; 16];
    time_bytes[0..8].copy_from_slice(&sec.to_ne_bytes());
    time_bytes[8..16].copy_from_slice(&usec.to_ne_bytes());
    let mut offset = 0;
    for buffer in buffers {
        let len = buffer.len();
        buffer.copy_from_slice(&time_bytes[offset..offset + len]);
        offset += len;
    }
    0
}

fn is_valid_sv39_va(addr: usize) -> bool {
    let top = addr >> 39;
    let sign = (addr >> 38) & 1;
    if sign == 0 {
        top == 0
    } else {
        top == 0x1FFFFFF
    }
}

/// read/write memory or query syscall count through page table
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    let token = current_user_token();
    match trace_request {
        0 => {
            // Read byte from user address with permission check
            if !is_valid_sv39_va(id) {
                return -1;
            }
            let page_table = PageTable::from_token(token);
            let va = VirtAddr::from(id);
            let vpn = va.floor();
            if let Some(pte) = page_table.translate(vpn) {
                if pte.is_valid() && pte.readable() {
                    let ppn = pte.ppn();
                    let pa = crate::mm::PhysAddr::from(ppn.0 << 12 | va.page_offset());
                    unsafe { *(pa.0 as *const u8) as isize }
                } else {
                    -1
                }
            } else {
                -1
            }
        }
        1 => {
            // Write byte to user address with permission check
            if !is_valid_sv39_va(id) {
                return -1;
            }
            let page_table = PageTable::from_token(token);
            let va = VirtAddr::from(id);
            let vpn = va.floor();
            if let Some(pte) = page_table.translate(vpn) {
                if pte.is_valid() && pte.writable() {
                    let ppn = pte.ppn();
                    let pa = crate::mm::PhysAddr::from(ppn.0 << 12 | va.page_offset());
                    unsafe { *(pa.0 as *mut u8) = data as u8; }
                    0
                } else {
                    -1
                }
            } else {
                -1
            }
        }
        2 => {
            // Query syscall count
            current_syscall_times(id) as isize
        }
        _ => -1,
    }
}

/// mmap: map physical frames to the given virtual address range
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap");
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    // start must be page-aligned
    if !start_va.aligned() {
        return -1;
    }
    // port validity check
    if port & !0x7 != 0 {
        return -1;
    }
    if port == 0 {
        return -1;
    }
    let mut map_perm = crate::mm::MapPermission::U;
    if port & 1 != 0 {
        map_perm |= crate::mm::MapPermission::R;
    }
    if port & 2 != 0 {
        map_perm |= crate::mm::MapPermission::W;
    }
    if port & 4 != 0 {
        map_perm |= crate::mm::MapPermission::X;
    }
    mmap_current(start_va, end_va, map_perm)
}

/// munmap: unmap the given virtual address range
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    munmap_current(start_va, end_va)
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
