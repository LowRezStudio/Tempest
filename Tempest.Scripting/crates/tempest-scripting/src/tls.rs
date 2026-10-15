//! Anti-cheat XOR key for `TAntiCheatArray` (port of WolperMod `EngineTls.cpp`).
//!
//! GObjects/GNames entries are stored as `Data[i] ^ key(i)`. The key table
//! lives in the game main thread's TLS block:
//!
//! ```text
//! tls_base = *(main_thread_TEB + 0x58)      // ThreadLocalStoragePointer
//! storage  = *tls_base                      // module TLS block
//! key(i)   = *(storage + 48 + 8 * (i & mask))
//! ```
//!
//! `mask` is the u32 immediate at `xor_helper + 0x65` (see `unreal::resolve`).

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// `u32::MAX` = unset.
static MASK: AtomicU32 = AtomicU32::new(u32::MAX);
/// `0` = not found yet (retried on demand; the table exists before GNames fills).
static TLS_BASE: AtomicUsize = AtomicUsize::new(0);

pub fn set_mask(mask: u32) {
    MASK.store(mask, Ordering::Release);
}

pub fn mask() -> Option<u32> {
    match MASK.load(Ordering::Acquire) {
        u32::MAX => None,
        m => Some(m),
    }
}

/// XOR key for array slot `i`.
pub fn xor_key(i: usize) -> Option<usize> {
    let mask = mask()? as usize;
    let base = tls_base()?;
    let storage: usize = unsafe { crate::memory::read(base)? };
    if storage == 0 {
        return None;
    }
    unsafe { crate::memory::read(storage + 48 + 8 * (i & mask)) }
}

fn tls_base() -> Option<usize> {
    let cached = TLS_BASE.load(Ordering::Acquire);
    if cached != 0 {
        return Some(cached);
    }
    let found = find_main_thread_tls()?;
    TLS_BASE.store(found, Ordering::Release);
    crate::log::line("tls", &format!("main-thread tls_base=0x{found:x}"));
    Some(found)
}

#[cfg(not(windows))]
fn find_main_thread_tls() -> Option<usize> {
    None
}

/// Earliest-created thread of this process → `NtQueryInformationThread`
/// (ThreadBasicInformation) → TEB → `TEB+0x58`.
#[cfg(windows)]
fn find_main_thread_tls() -> Option<usize> {
    use std::ffi::c_void;
    const TH32CS_SNAPTHREAD: u32 = 0x4;
    const THREAD_QUERY_INFORMATION: u32 = 0x40;
    const TEB_TLS_POINTER: usize = 0x58;

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot.is_null() || snapshot as isize == -1 {
            return None;
        }
        let pid = GetCurrentProcessId();
        let mut entry: ThreadEntry32 = std::mem::zeroed();
        entry.size = std::mem::size_of::<ThreadEntry32>() as u32;
        let mut main_tid = 0u32;
        let mut earliest = u64::MAX;
        let mut ok = Thread32First(snapshot, &mut entry) != 0;
        while ok {
            if entry.owner_pid == pid {
                let h = OpenThread(THREAD_QUERY_INFORMATION, 0, entry.tid);
                if !h.is_null() {
                    let mut times = [FileTime { lo: 0, hi: 0 }; 4];
                    let (c, e, k, u) = {
                        let [c, e, k, u] = &mut times;
                        (c as *mut _, e as *mut _, k as *mut _, u as *mut _)
                    };
                    if GetThreadTimes(h, c, e, k, u) != 0 {
                        let ts = ((times[0].hi as u64) << 32) | times[0].lo as u64;
                        if ts != 0 && ts < earliest {
                            earliest = ts;
                            main_tid = entry.tid;
                        }
                    }
                    CloseHandle(h);
                }
            }
            ok = Thread32Next(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        if main_tid == 0 {
            return None;
        }

        let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr() as *const u8);
        let query = GetProcAddress(ntdll, c"NtQueryInformationThread".as_ptr() as *const u8);
        if query.is_null() {
            return None;
        }
        let query: NtQueryInformationThreadFn = std::mem::transmute(query);
        let thread = OpenThread(THREAD_QUERY_INFORMATION, 0, main_tid);
        if thread.is_null() {
            return None;
        }
        let mut info: ThreadBasicInformation = std::mem::zeroed();
        let mut len = 0u32;
        let status = query(
            thread,
            0,
            &mut info as *mut _ as *mut c_void,
            std::mem::size_of::<ThreadBasicInformation>() as u32,
            &mut len,
        );
        CloseHandle(thread);
        if status != 0 || info.teb == 0 {
            return None;
        }
        let tls: usize = crate::memory::read(info.teb + TEB_TLS_POINTER)?;
        if tls == 0 {
            None
        } else {
            Some(tls)
        }
    }
}

#[cfg(windows)]
type NtQueryInformationThreadFn = unsafe extern "system" fn(
    thread: *mut std::ffi::c_void,
    class: i32,
    info: *mut std::ffi::c_void,
    len: u32,
    ret_len: *mut u32,
) -> i32;

#[cfg(windows)]
#[repr(C)]
struct ThreadEntry32 {
    size: u32,
    usage: u32,
    tid: u32,
    owner_pid: u32,
    base_pri: i32,
    delta_pri: i32,
    flags: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct FileTime {
    lo: u32,
    hi: u32,
}

#[cfg(windows)]
#[repr(C)]
struct ThreadBasicInformation {
    exit_status: i32,
    teb: usize,
    client_id: [usize; 2],
    affinity_mask: usize,
    priority: i32,
    base_priority: i32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut std::ffi::c_void;
    fn Thread32First(snapshot: *mut std::ffi::c_void, entry: *mut ThreadEntry32) -> i32;
    fn Thread32Next(snapshot: *mut std::ffi::c_void, entry: *mut ThreadEntry32) -> i32;
    fn OpenThread(access: u32, inherit: i32, tid: u32) -> *mut std::ffi::c_void;
    fn GetThreadTimes(
        thread: *mut std::ffi::c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    fn GetCurrentProcessId() -> u32;
    fn GetModuleHandleA(name: *const u8) -> *mut std::ffi::c_void;
    fn GetProcAddress(module: *mut std::ffi::c_void, name: *const u8) -> *mut std::ffi::c_void;
}
