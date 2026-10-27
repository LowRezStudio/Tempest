//! Raw process memory: module lookup, reads/writes, signature scanning, and
//! the bits of x86 / x86-64 operand decoding the signatures need.
//!
//! On non-Windows hosts every read returns `None` and every scan is empty, so
//! the rest of the crate (and `cargo test`) runs without a game image.

/// The main executable image.
#[derive(Debug, Clone, Copy)]
pub struct Module {
    pub base: usize,
    pub size: usize,
}

/// `GetModuleHandle(NULL)` + `OptionalHeader.SizeOfImage`.
pub fn main_module() -> Option<Module> {
    #[cfg(windows)]
    unsafe {
        let base = GetModuleHandleA(std::ptr::null()) as usize;
        if base == 0 {
            return None;
        }
        let e_lfanew: u32 = read(base + 0x3C)?;
        let nt = base + e_lfanew as usize;
        if read::<u32>(nt)? != 0x0000_4550 {
            return None;
        }
        // OptionalHeader starts at nt+24; SizeOfImage is at +56 in both
        // PE32 and PE32+.
        let size: u32 = read(nt + 24 + 56)?;
        Some(Module {
            base,
            size: size as usize,
        })
    }
    #[cfg(not(windows))]
    None
}

// ---------------------------------------------------------------------------
// Reads / writes
// ---------------------------------------------------------------------------

/// Volatile read. `None` for a null address or on the host build.
///
/// # Safety
/// `addr` must be readable for `T`; callers only pass pointers obtained from
/// the game's own structures.
pub unsafe fn read<T: Copy>(addr: usize) -> Option<T> {
    if addr == 0 {
        return None;
    }
    #[cfg(windows)]
    {
        Some(std::ptr::read_volatile(addr as *const T))
    }
    #[cfg(not(windows))]
    None
}

/// `read` for addresses nothing vouches for (script input): `None` when any
/// byte is unreadable, instead of faulting. One `ReadProcessMemory` on this
/// process per call.
pub fn try_read<T: Copy>(addr: usize) -> Option<T> {
    if addr == 0 {
        return None;
    }
    #[cfg(windows)]
    unsafe {
        let size = std::mem::size_of::<T>();
        let mut value = std::mem::MaybeUninit::<T>::uninit();
        let mut copied = 0usize;
        let ok = ReadProcessMemory(GetCurrentProcess(), addr as *const _, value.as_mut_ptr().cast(), size, &mut copied);
        (ok != 0 && copied == size).then(|| value.assume_init())
    }
    #[cfg(not(windows))]
    None
}

/// Volatile write. `false` for a null address or on the host build.
///
/// # Safety
/// `addr` must be writable for `T`.
pub unsafe fn write<T: Copy>(addr: usize, value: T) -> bool {
    if addr == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        std::ptr::write_volatile(addr as *mut T, value);
        true
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        false
    }
}

/// The code units of a NUL-terminated string at `addr`, at most `max` of them,
/// each read with `read_unit`; `None` when one is unreadable.
fn read_units<U: Copy + Default + PartialEq>(addr: usize, max: usize, read_unit: impl Fn(usize) -> Option<U>) -> Option<Vec<U>> {
    let mut out = Vec::new();
    for i in 0..max {
        let unit = read_unit(addr + i * std::mem::size_of::<U>())?;
        if unit == U::default() {
            break;
        }
        out.push(unit);
    }
    Some(out)
}

/// NUL-terminated ANSI string, at most `max` bytes.
pub fn read_cstr(addr: usize, max: usize) -> Option<String> {
    read_units(addr, max, |a| unsafe { read::<u8>(a) }).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// NUL-terminated UTF-16 string, at most `max` code units.
pub fn read_wstr(addr: usize, max: usize) -> Option<String> {
    read_units(addr, max, |a| unsafe { read::<u16>(a) }).map(|w| String::from_utf16_lossy(&w))
}

/// `read_cstr` through `try_read`.
pub fn try_read_cstr(addr: usize, max: usize) -> Option<String> {
    read_units(addr, max, try_read::<u8>).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// `read_wstr` through `try_read`.
pub fn try_read_wstr(addr: usize, max: usize) -> Option<String> {
    read_units(addr, max, try_read::<u16>).map(|w| String::from_utf16_lossy(&w))
}

// ---------------------------------------------------------------------------
// Signature scanning
// ---------------------------------------------------------------------------

/// `"48 8B 05 ? ? ? ? 4C"` → bytes with `None` wildcards.
pub fn parse_pattern(pattern: &str) -> Result<Vec<Option<u8>>, String> {
    let mut out = Vec::new();
    for tok in pattern.split_whitespace() {
        match tok {
            "?" | "??" => out.push(None),
            t if t.len() == 2 => out.push(Some(
                u8::from_str_radix(t, 16).map_err(|_| format!("bad byte `{t}`"))?,
            )),
            t => return Err(format!("bad token `{t}`")),
        }
    }
    if out.is_empty() {
        return Err("empty pattern".into());
    }
    Ok(out)
}

/// First match of `pattern` in the module, as an absolute address.
pub fn scan(module: Module, pattern: &[Option<u8>]) -> Option<usize> {
    scan_all(module, pattern, 1).into_iter().next()
}

/// Up to `limit` matches, absolute addresses, in address order. Only
/// committed, readable pages are touched (the image can contain reserved
/// or guarded ranges that would fault on a naive `SizeOfImage` sweep).
pub fn scan_all(module: Module, pattern: &[Option<u8>], limit: usize) -> Vec<usize> {
    let mut hits = Vec::new();
    for (start, len) in readable_regions(module) {
        let bytes = unsafe { std::slice::from_raw_parts(start as *const u8, len) };
        for off in find_all(bytes, pattern, limit - hits.len()) {
            hits.push(start + off);
        }
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

fn find_all(hay: &[u8], pattern: &[Option<u8>], limit: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if pattern.is_empty() || pattern.len() > hay.len() || limit == 0 {
        return out;
    }
    let first = pattern[0];
    let last = hay.len() - pattern.len();
    let mut i = 0;
    while i <= last {
        if let Some(b) = first {
            match hay[i..=last].iter().position(|&x| x == b) {
                Some(p) => i += p,
                None => break,
            }
        }
        let ok = pattern
            .iter()
            .enumerate()
            .all(|(j, p)| p.is_none_or(|b| hay[i + j] == b));
        if ok {
            out.push(i);
            if out.len() >= limit {
                break;
            }
        }
        i += 1;
    }
    out
}

/// Is `[addr, addr+len)` inside one committed, readable, non-guard region?
/// Used to vet pointers derived from decryption before the first deref.
pub fn is_readable(addr: usize, len: usize) -> bool {
    #[cfg(windows)]
    unsafe {
        const MEM_COMMIT: u32 = 0x1000;
        const PAGE_NOACCESS: u32 = 0x01;
        const PAGE_GUARD: u32 = 0x100;
        let mut info: MemoryBasicInformation = std::mem::zeroed();
        let n = VirtualQuery(
            addr as *const _,
            &mut info,
            std::mem::size_of::<MemoryBasicInformation>(),
        );
        n != 0
            && info.state == MEM_COMMIT
            && info.protect != 0
            && info.protect & (PAGE_NOACCESS | PAGE_GUARD) == 0
            && addr + len <= info.base_address + info.region_size
    }
    #[cfg(not(windows))]
    {
        let _ = (addr, len);
        false
    }
}

/// First match of `pattern` inside `[start, start+len)` (must be readable;
/// used to look inside an already-resolved function body).
pub fn scan_range(start: usize, len: usize, pattern: &[Option<u8>]) -> Option<usize> {
    if start == 0 || !is_readable(start, len) {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(start as *const u8, len) };
    find_all(bytes, pattern, 1).first().map(|off| start + off)
}

/// `(start, len)` of each committed, readable, non-guard region of the image.
fn readable_regions(module: Module) -> Vec<(usize, usize)> {
    #[cfg(windows)]
    unsafe {
        const MEM_COMMIT: u32 = 0x1000;
        const PAGE_NOACCESS: u32 = 0x01;
        const PAGE_GUARD: u32 = 0x100;
        let end = module.base + module.size;
        let mut regions = Vec::new();
        let mut addr = module.base;
        while addr < end {
            let mut info: MemoryBasicInformation = std::mem::zeroed();
            let n = VirtualQuery(
                addr as *const _,
                &mut info,
                std::mem::size_of::<MemoryBasicInformation>(),
            );
            if n == 0 || info.region_size == 0 {
                break;
            }
            let region_end = (info.base_address + info.region_size).min(end);
            let readable = info.state == MEM_COMMIT
                && info.protect & (PAGE_NOACCESS | PAGE_GUARD) == 0
                && info.protect != 0;
            if readable && region_end > addr {
                regions.push((addr, region_end - addr));
            }
            addr = region_end;
        }
        regions
    }
    #[cfg(not(windows))]
    {
        let _ = module;
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Operand decoding (x64: rip-relative disp32; x86: absolute disp32)
// ---------------------------------------------------------------------------

/// `mov reg, [rip+disp32]` (7 bytes, disp at +3) → address of the global.
pub fn rip_target(instr: usize) -> Option<usize> {
    rel_target(instr, 3, 7)
}

/// `call rel32` (5 bytes, disp at +1) → call target.
pub fn call_target(instr: usize) -> Option<usize> {
    rel_target(instr, 1, 5)
}

/// A rip-/eip-relative displacement at `instr + disp_at` of an instruction
/// `len` bytes long → the absolute target.
pub fn rel_target(instr: usize, disp_at: usize, len: usize) -> Option<usize> {
    let disp: i32 = unsafe { read(instr + disp_at)? };
    Some((instr as isize + len as isize + disp as isize) as usize)
}

/// The 32-bit absolute address embedded at `instr + at` (x86 `mov reg,
/// [disp32]`, `push imm32`, `[base + index*4 + disp32]`, ...).
pub fn abs32_at(instr: usize, at: usize) -> Option<usize> {
    let v: u32 = unsafe { read(instr + at)? };
    Some(v as usize)
}

/// The function entry (`push ebp; mov ebp, esp` or `push ebp; mov ebp, esp;
/// push -1` SEH prologue, or a 16-byte aligned `int3` run before it) at or
/// before `addr`, searching back at most `max_back` bytes. x86 only; x64
/// functions are found through call sites instead.
pub fn function_start_before(addr: usize, max_back: usize) -> Option<usize> {
    let start = addr.checked_sub(max_back)?;
    if !is_readable(start, max_back + 1) {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(start as *const u8, max_back + 1) };
    (0..=max_back).rev().find(|&i| bytes[i..].starts_with(&[0x55, 0x8B, 0xEC])).map(|i| start + i)
}

/// `MEMORY_BASIC_INFORMATION`: 48 bytes on x64 (PartitionId + padding), 28 on x86.
#[cfg(all(windows, target_pointer_width = "64"))]
#[repr(C)]
struct MemoryBasicInformation {
    base_address: usize,
    allocation_base: usize,
    allocation_protect: u32,
    partition_id: u16,
    _pad0: u16,
    region_size: usize,
    state: u32,
    protect: u32,
    kind: u32,
    _pad1: u32,
}

#[cfg(all(windows, target_pointer_width = "32"))]
#[repr(C)]
struct MemoryBasicInformation {
    base_address: usize,
    allocation_base: usize,
    allocation_protect: u32,
    region_size: usize,
    state: u32,
    protect: u32,
    kind: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleA(name: *const u8) -> *mut std::ffi::c_void;
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn ReadProcessMemory(
        process: *mut std::ffi::c_void,
        addr: *const std::ffi::c_void,
        buffer: *mut std::ffi::c_void,
        size: usize,
        copied: *mut usize,
    ) -> i32;
    fn VirtualQuery(
        addr: *const std::ffi::c_void,
        info: *mut MemoryBasicInformation,
        len: usize,
    ) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_parse() {
        let p = parse_pattern("48 8B 05 ? ?? 4C").unwrap();
        assert_eq!(p, vec![Some(0x48), Some(0x8B), Some(0x05), None, None, Some(0x4C)]);
        assert!(parse_pattern("").is_err());
        assert!(parse_pattern("zz").is_err());
        assert!(parse_pattern("123").is_err());
    }

    #[test]
    fn find_all_matches_wildcards_and_limits() {
        let hay = b"\x90\x48\x8B\x05\x11\x22\x48\x8B\x05\x33";
        let p = parse_pattern("48 8B 05 ?").unwrap();
        assert_eq!(find_all(hay, &p, 10), vec![1, 6]);
        assert_eq!(find_all(hay, &p, 1), vec![1]);
        assert!(find_all(hay, &parse_pattern("FF").unwrap(), 10).is_empty());
        assert!(find_all(b"\x01", &parse_pattern("01 02").unwrap(), 10).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn try_read_returns_none_instead_of_faulting() {
        let value: u32 = 0xDEAD_BEEF;
        assert_eq!(try_read::<u32>(&value as *const u32 as usize), Some(0xDEAD_BEEF));
        assert_eq!(try_read::<u32>(0x10), None);
        let text = b"hi\0";
        assert_eq!(try_read_cstr(text.as_ptr() as usize, 16).as_deref(), Some("hi"));
        assert_eq!(try_read_cstr(0x10, 16), None);
    }

    #[test]
    fn memory_basic_information_matches_the_sdk() {
        #[cfg(all(windows, target_pointer_width = "64"))]
        assert_eq!(std::mem::size_of::<MemoryBasicInformation>(), 48);
        #[cfg(all(windows, target_pointer_width = "32"))]
        assert_eq!(std::mem::size_of::<MemoryBasicInformation>(), 28);
    }
}
