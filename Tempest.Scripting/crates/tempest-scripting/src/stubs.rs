//! Executable entry stubs, generated at runtime for the hook machinery.
//!
//! A detour or a swapped `UFunction.Func` lands on a bare function pointer
//! with no room for context, so every slot gets its own tiny stub:
//! `mov r10d, <slot>; mov rax, <common>; jmp rax`. The shared `common` entry
//! then knows which slot fired from R10. Stubs are written into RWX pages
//! allocated on demand, so there is no fixed number of them; they are never
//! freed (a stub may be executing on another thread at any moment).

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::sync::Mutex;

    const SIZE: usize = 32;
    const PAGE: usize = 4096;
    /// Current page and bytes used in it.
    static CURRENT: Mutex<(usize, usize)> = Mutex::new((0, PAGE));

    pub fn make(slot: usize, common: usize) -> Result<usize, String> {
        let slot32 = u32::try_from(slot).map_err(|_| "too many hook slots".to_string())?;
        let mut cur = CURRENT.lock().unwrap();
        if cur.1 + SIZE > PAGE {
            const MEM_COMMIT_RESERVE: u32 = 0x1000 | 0x2000;
            const PAGE_EXECUTE_READWRITE: u32 = 0x40;
            let page = unsafe { VirtualAlloc(std::ptr::null_mut(), PAGE, MEM_COMMIT_RESERVE, PAGE_EXECUTE_READWRITE) };
            if page.is_null() {
                return Err("VirtualAlloc for a hook stub page failed".into());
            }
            *cur = (page as usize, 0);
        }
        let stub = cur.0 + cur.1;
        let mut code = Vec::with_capacity(SIZE);
        code.extend_from_slice(&[0x41, 0xBA]); // mov r10d, imm32
        code.extend_from_slice(&slot32.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xB8]); // mov rax, imm64
        code.extend_from_slice(&(common as u64).to_le_bytes());
        code.extend_from_slice(&[0xFF, 0xE0]); // jmp rax
        code.resize(SIZE, 0xCC);
        // SAFETY: `stub` is inside a page this module allocated RWX and has not handed out yet.
        unsafe {
            std::ptr::copy_nonoverlapping(code.as_ptr(), stub as *mut u8, SIZE);
            FlushInstructionCache(GetCurrentProcess(), stub as *const c_void, SIZE);
        }
        cur.1 += SIZE;
        Ok(stub)
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn VirtualAlloc(addr: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
        fn FlushInstructionCache(process: *mut c_void, addr: *const c_void, size: usize) -> i32;
        fn GetCurrentProcess() -> *mut c_void;
    }
}

/// A stub for `slot` that jumps to `common` with the slot id in R10.
#[cfg(windows)]
pub fn make(slot: usize, common: usize) -> Result<usize, String> {
    imp::make(slot, common)
}

#[cfg(not(windows))]
pub fn make(_slot: usize, _common: usize) -> Result<usize, String> {
    Err("host build".into())
}
