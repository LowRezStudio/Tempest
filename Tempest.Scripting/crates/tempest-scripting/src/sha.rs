//! Turns off the client's package SHA-1 check, so mods can replace the
//! script packages the exe lists (`Startup.upk`, `TgClient.upk`, ...).
//!
//! The client hashes those files as it loads them and, on a mismatch, calls
//! `appOnFailSHAVerification(path, bMissingHash)`, which only formats
//! "--PLEASE ATTEMPT TO REPAIR YOUR INSTALL-- SHA Verification failed ..."
//! and raises a fatal `appErrorf`. Both callers simply carry on (cleanup)
//! when it returns, so writing a `ret` over its first byte makes a mismatch
//! harmless. The launcher's alternative, writing the new hashes into the
//! started game, needs `WriteProcessMemory` from a native Windows launcher;
//! doing it in here works the same under Wine/Proton and on both clients.
//!
//! The check fires seconds after the startup packages load (13 s into a
//! Proton launch), while injection happens with only `Core` loaded, so
//! [`disable`] runs first thing in boot.

use crate::memory;

/// The whole function: two `cmov`s pick the reason and path, then a tail
/// call into `GError->Logf`. The format string's `lea rdx` is at +0x29.
#[cfg(target_pointer_width = "64")]
const SIG: &str = "85 D2 48 8D 05 ?? ?? ?? ?? 4C 8D 0D ?? ?? ?? ?? 4C 0F 45 C8 48 85 C9 4C 8D 05 ?? ?? ?? ?? 4C 0F 45 C1 48 8B 0D ?? ?? ?? ?? 48 8D 15 ?? ?? ?? ?? E9";
/// cdecl, same shape; the format string's `push imm32` is at +0x22.
#[cfg(target_pointer_width = "32")]
const SIG: &str = "83 7C 24 08 00 B9 ?? ?? ?? ?? B8 ?? ?? ?? ?? 0F 44 C1 50 8B 44 24 08 85 C0 B9 ?? ?? ?? ?? 0F 45 C8 51 68 ?? ?? ?? ?? FF 35 ?? ?? ?? ?? E8";

/// The format string the function passes, to be sure the signature found it.
fn format_string(func: usize) -> Option<String> {
    #[cfg(target_pointer_width = "64")]
    let at = memory::rel_target(func + 0x29, 3, 7)?;
    #[cfg(target_pointer_width = "32")]
    let at = memory::abs32_at(func + 0x22, 1)?;
    memory::try_read_wstr(at, 128)
}

fn log(msg: &str) {
    crate::log::line("sha", msg);
}

/// Patches `appOnFailSHAVerification` to return at once. Logs and does
/// nothing when the signature or its format string doesn't match exactly.
pub fn disable() {
    let Some(m) = memory::main_module() else { return };
    let pat = memory::parse_pattern(SIG).expect("built-in signature parses");
    let hits = memory::scan_all(m, &pat, 2);
    if hits.len() != 1 {
        log(&format!("disabled: signature appOnFailSHAVerification matched {} times", hits.len()));
        return;
    }
    let func = hits[0];
    match format_string(func) {
        Some(s) if s.contains("SHA Verification failed") => {}
        other => {
            log(&format!("disabled: 0x{func:x} does not use the SHA failure message ({other:?})"));
            return;
        }
    }
    if unsafe { memory::read::<u8>(func) } == Some(0xC3) {
        log(&format!("check already off (0x{func:x})"));
        return;
    }
    if patch_ret(func) {
        log(&format!("package SHA-1 check off: appOnFailSHAVerification at 0x{func:x} returns at once"));
    } else {
        log(&format!("could not patch appOnFailSHAVerification at 0x{func:x}"));
    }
}

#[cfg(windows)]
fn patch_ret(func: usize) -> bool {
    use std::ffi::c_void;
    const PAGE_EXECUTE_READWRITE: u32 = 0x40;
    #[link(name = "kernel32")]
    extern "system" {
        fn VirtualProtect(addr: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
        fn FlushInstructionCache(process: *mut c_void, addr: *const c_void, size: usize) -> i32;
        fn GetCurrentProcess() -> *mut c_void;
    }
    // SAFETY: `func` is the verified entry of a function in the main image;
    // one byte becomes `ret`, which suits both its conventions (x64 and
    // cdecl: the caller cleans the stack).
    unsafe {
        let mut old = 0u32;
        if VirtualProtect(func as *mut c_void, 1, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        std::ptr::write_volatile(func as *mut u8, 0xC3);
        VirtualProtect(func as *mut c_void, 1, old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), func as *const c_void, 1);
    }
    true
}

#[cfg(not(windows))]
fn patch_ret(_func: usize) -> bool {
    false
}
