//! UE3 object model for the Paladins 8.1 Win64 client.
//!
//! What is baked in, and where it was verified:
//!
//! * The five bootstrap signatures and the TLS-XOR decrypt come verbatim
//!   from WolperMod (`runtime/src/Runtime/ModuleEntry.cpp`, `EngineTls.cpp`),
//!   a working client mod for this exact build.
//! * Engine-core layouts (`UObject`, `FName`, `FNameEntry`, `TArray`,
//!   `TAntiCheatArray`, `UField`/`UStruct`/`UProperty`) match the 8.1 client
//!   SDK (`WolperMod/runtime/SDK/PL_Basic.hpp`, `PL_Core_classes.hpp`) and
//!   the 8.2 server debug types in Binary Ninja. They are identical between
//!   the two builds, unlike game-class field offsets, which is why property
//!   access walks live `UProperty` reflection instead of hardcoding offsets.
//!
//! Layout cheat sheet (x64):
//!
//! ```text
//! TAntiCheatArray   Data@0x00  Unk00@0x08  Count@0x0C   (client only; 8-byte
//!                   XOR-encrypted entries. Count@0x0C is live-verified on 8.1;
//!                   the 8.2 server uses a plain TArray, do not copy its layout)
//! TArray            Data@0x00  Count@0x08  Max@0x0C
//! FName             Index@0x00 Number@0x04
//! FNameEntry        Flags@0x00 Index@0x08 (bit0 = wide)  Name@0x14
//! UObject           Outer@0x40 Name@0x48 Class@0x50
//! UField            Next@0x60
//! UStruct           SuperField@0x78 Children@0x80
//! UStruct           PropertySize@0x88 (UFunction: params frame size)
//! UFunction         FunctionFlags@0xD0
//! UProperty         ArrayDim@0x68 ElementSize@0x6C Flags@0x70 Offset@0x7C
//! U*Property        BitMask / PropertyClass / Struct / Inner @0x98
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::memory;

pub const SIG_GOBJECTS: &str = "48 8B 05 ? ? ? ? 4C 8B 04 0A 4C 33 04 F0 44";
pub const SIG_GNAMES: &str = "48 8B 05 ? ? ? ? B9 ? ? ? ? 48 8B 0C 0A 48 BA";
pub const SIG_ENGINE: &str = "48 8B 0D ? ? ? ? 48 85 C9 0F 84 ? ? ? ? 48 39 B9 ? ? ? ?";
pub const SIG_PROCESS_EVENT: &str = "E8 ? ? ? ? 8D 4E ? 41 3B CD";
pub const SIG_XOR: &str = "E8 ? ? ? ? 85 C0 75 0F 48 8B 03";
/// Inside `appRealloc`: `mov rcx,[rip+GMalloc]; mov rax,[rcx]; mov r9d,ebx;
/// mov r8d,edi; mov rdx,rsi` right before `jmp [rax+0x18]` (FMalloc::Realloc).
pub const SIG_GMALLOC: &str = "48 8B 0D ? ? ? ? 48 8B 01 44 8B CB 44 8B C7 48 8B D6";
/// Inside `ProcessEvent`: `lea r13,[rip+GNatives]` before the optional-parm loop.
pub const SIG_GNATIVES_LEA: &str = "4C 8D 2D ? ? ? ?";
/// Inside `ProcessEvent`: `or dword [rip+GRuntimeUCFlags], 2` (RUC_SkippedOptionalParm).
pub const SIG_RUNTIME_UC_FLAGS: &str = "83 0D ? ? ? ? 02";
/// WolperMod: `ReadImmediate32(xorFunction + 0x63)` reads the u32 at `+2`.
const XOR_MASK_IMM: usize = 0x63 + 2;

pub mod layout {
    pub const CHEAT_ARRAY_COUNT: usize = 0x0C;
    pub const TARRAY_COUNT: usize = 0x08;
    pub const TARRAY_MAX: usize = 0x0C;
    pub const FNAME_ENTRY_INDEX: usize = 0x08;
    pub const FNAME_ENTRY_NAME: usize = 0x14;
    pub const UOBJECT_OUTER: usize = 0x40;
    pub const UOBJECT_NAME: usize = 0x48;
    pub const UOBJECT_CLASS: usize = 0x50;
    pub const UFIELD_NEXT: usize = 0x60;
    pub const USTRUCT_SUPER: usize = 0x78;
    pub const USTRUCT_CHILDREN: usize = 0x80;
    pub const UPROP_ELEMENT_SIZE: usize = 0x6C;
    pub const UPROP_FLAGS: usize = 0x70;
    pub const UPROP_OFFSET: usize = 0x7C;
    pub const UPROP_EXT: usize = 0x98;
    /// `UMapProperty.Key` / `.Value` (8.1 SDK: 0x10 bytes after UProperty).
    pub const UMAP_KEY: usize = 0x98;
    pub const UMAP_VALUE: usize = 0xA0;
    /// UE3 `TMap` = `TSet<TPair>` over a sparse array (8.1 SDK `PL_Basic.hpp`).
    pub const TMAP_ELEMENTS: usize = 0x00;
    pub const TMAP_COUNT: usize = 0x08;
    pub const TMAP_INLINE_BITS: usize = 0x10;
    pub const TMAP_INDIRECT_BITS: usize = 0x20;
    /// `UStruct.PropertySize`: for a `UFunction`, the size of its params frame.
    pub const USTRUCT_PROPERTY_SIZE: usize = 0x88;
    pub const UFUNCTION_FLAGS: usize = 0xD0;
    /// `UFunction.Script` (`TArray<BYTE>`): natives hold an `EX_NativeParm`
    /// stream here so they can run against a params frame.
    pub const UFUNCTION_SCRIPT_DATA: usize = 0x8C;
    pub const UFUNCTION_SCRIPT_NUM: usize = 0x94;
    pub const UFUNCTION_PARMS_SIZE: usize = 0xE2;
    pub const UFUNCTION_RETURN_OFFSET: usize = 0xE4;
    /// `void (UObject::*Func)(FFrame&, void*)`: `ProcessInternal` for script
    /// functions, the native implementation otherwise.
    pub const UFUNCTION_FUNC: usize = 0xF0;
    /// `FFrame` (8.1 client, 4-byte packed; from the ProcessEvent disassembly).
    pub const FRAME_NODE: usize = 0x14;
    pub const FRAME_OBJECT: usize = 0x1C;
    pub const FRAME_CODE: usize = 0x24;
    pub const FRAME_LOCALS: usize = 0x2C;
    pub const FRAME_PREVIOUS: usize = 0x34;
    pub const FRAME_OUT_PARMS: usize = 0x3C;
    pub const FRAME_SIZE: usize = 0x48;
    /// Bytecode opcodes, from the 8.2 `GRegisterNative` static initializer
    /// (standard UE3 numbering; the thunk relies on the first three).
    pub const EX_END_FUNCTION_PARMS: u8 = 0x16;
    pub const EX_NATIVE_PARM: u8 = 0x29;
    pub const EX_EMPTY_PARM_VALUE: u8 = 0x4A;
    pub const EX_NOTHING: u8 = 0x0B;
    pub const EX_NEW: u8 = 0x11;
    pub const EX_OBJECT_CONST: u8 = 0x20;
    pub const EX_STRING_CONST: u8 = 0x1F;
    /// `FOutParmRec { UProperty* Property; BYTE* PropAddr; FOutParmRec* NextOutParm; }`
    pub const OUTPARM_PROPERTY: usize = 0x00;
    pub const OUTPARM_ADDR: usize = 0x08;
    pub const OUTPARM_NEXT: usize = 0x10;
    pub const OUTPARM_SIZE: usize = 0x18;
    /// `FMalloc` vtable slots (`appRealloc` → `+0x18`, `appFree` → `+0x20`).
    pub const GMALLOC_REALLOC: usize = 0x18;
    pub const GMALLOC_FREE: usize = 0x20;
    pub const CPF_OPTIONAL_PARM: u64 = 0x10;
    pub const CPF_PARM: u64 = 0x80;
    pub const CPF_OUT_PARM: u64 = 0x100;
    pub const CPF_RETURN_PARM: u64 = 0x400;
    pub const FUNC_NATIVE: u32 = 0x400;
    /// `ProcessEvent` runs the per-optional-parm default prologue when set.
    pub const FUNC_HAS_OPTIONAL_PARMS: u32 = 0x404000;
    /// `GRuntimeUCFlags` bit: the caller skipped the optional parm being evaluated.
    pub const RUC_SKIPPED_OPTIONAL_PARM: u32 = 0x2;
}
use layout::*;

/// How long `resolve` waits for the game to fill GNames/GObjects after the
/// signatures resolved (the launcher injects ~1 s after process start).
const READY_TIMEOUT: Duration = Duration::from_secs(90);

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

/// Resolved addresses. `0` = unresolved; `notes` says why.
#[derive(Debug, Clone, Default)]
pub struct Globals {
    /// `&GObjects` (the `TAntiCheatArray<UObject*>` itself).
    pub gobjects: usize,
    /// `&GNames` (the `TAntiCheatArray<FNameEntry*>` itself).
    pub gnames: usize,
    /// `&GEngine` (slot holding the `UEngine*`).
    pub engine_slot: usize,
    pub process_event: usize,
    /// `UObject::ProcessInternal` (script bytecode executor), from `UFunction.Func`.
    pub process_internal: usize,
    /// `&GMalloc` (slot holding the `FMalloc*`).
    pub gmalloc: usize,
    /// `GNatives[]` (opcode → exec function table).
    pub gnatives: usize,
    /// `&GRuntimeUCFlags` (optional-parm bookkeeping between caller and callee).
    pub runtime_uc_flags: usize,
    /// `&GPropAddr` (l-value address left behind by variable execs); found by probe.
    pub gpropaddr: usize,
    pub xor_mask: Option<u32>,
    pub script_hook_installed: bool,
    /// `GNames[0] == "None"` — proves signature + TLS + mask + entry layout.
    pub gnames_ok: bool,
    /// A decrypted object with a readable class and name exists.
    pub gobjects_ok: bool,
    pub hook_installed: bool,
    pub notes: Vec<String>,
}

static GLOBALS: Mutex<Option<Globals>> = Mutex::new(None);

/// Last resolved state (never scans).
pub fn globals() -> Globals {
    GLOBALS.lock().unwrap().clone().unwrap_or_default()
}

fn store(g: &Globals) {
    *GLOBALS.lock().unwrap() = Some(g.clone());
}

/// Scan the signatures, then wait until the name/object arrays are live.
/// Logs every step; returns what it got (also stored for `globals()`).
pub fn resolve() -> Globals {
    let mut g = Globals::default();
    match memory::main_module() {
        Some(m) => scan_signatures(m, &mut g),
        None => g.notes.push("no main module (host build?)".into()),
    }
    store(&g);
    if g.gnames != 0 && g.xor_mask.is_some() {
        wait_until_ready(&mut g);
    }
    crate::log::line(
        "resolve",
        &format!(
            "done gnames_ok={} gobjects_ok={} objects={} notes=[{}]",
            g.gnames_ok,
            g.gobjects_ok,
            object_count().unwrap_or(0),
            g.notes.join("; ")
        ),
    );
    store(&g);
    g
}

fn scan_signatures(m: memory::Module, g: &mut Globals) {
    crate::log::line(
        "resolve",
        &format!("module base=0x{:x} size=0x{:x}", m.base, m.size),
    );
    let find = |name: &str, sig: &str| -> Option<usize> {
        let pat = memory::parse_pattern(sig).expect("built-in signature parses");
        let hit = memory::scan(m, &pat);
        crate::log::line(
            "resolve",
            &format!(
                "{name}: {}",
                hit.map(|a| format!("0x{a:x}")).unwrap_or_else(|| "NOT FOUND".into())
            ),
        );
        hit
    };

    if let Some(instr) = find("xor", SIG_XOR) {
        match memory::call_target(instr).and_then(|f| unsafe { memory::read::<u32>(f + XOR_MASK_IMM) }) {
            Some(mask) if mask != 0 && (mask + 1).is_power_of_two() => {
                crate::log::line("resolve", &format!("xor mask=0x{mask:X}"));
                crate::tls::set_mask(mask);
                g.xor_mask = Some(mask);
            }
            Some(mask) => g.notes.push(format!("xor mask 0x{mask:X} is not (2^n)-1; patch drift?")),
            None => g.notes.push("xor helper unreadable".into()),
        }
    } else {
        g.notes.push("xor signature not found".into());
    }

    for (name, sig, slot) in [
        ("gobjects", SIG_GOBJECTS, &mut g.gobjects),
        ("gnames", SIG_GNAMES, &mut g.gnames),
        ("engine", SIG_ENGINE, &mut g.engine_slot),
    ] {
        match find(name, sig).and_then(memory::rip_target) {
            Some(addr) => {
                crate::log::line("resolve", &format!("{name} global at 0x{addr:x}"));
                *slot = addr;
            }
            None => g.notes.push(format!("{name} signature not found")),
        }
    }

    match find("processevent", SIG_PROCESS_EVENT).and_then(memory::call_target) {
        Some(addr) => {
            crate::log::line("resolve", &format!("ProcessEvent at 0x{addr:x}"));
            g.process_event = addr;
        }
        None => g.notes.push("processevent signature not found".into()),
    }

    match find("gmalloc", SIG_GMALLOC).and_then(memory::rip_target) {
        Some(slot) => {
            crate::log::line("resolve", &format!("GMalloc slot at 0x{slot:x}"));
            g.gmalloc = slot;
        }
        None => g.notes.push("gmalloc signature not found (FString growth disabled)".into()),
    }

    if g.process_event != 0 {
        let pat = memory::parse_pattern(SIG_GNATIVES_LEA).expect("built-in signature parses");
        match memory::scan_range(g.process_event, 0x400, &pat).and_then(memory::rip_target) {
            Some(table) => {
                crate::log::line("resolve", &format!("GNatives at 0x{table:x}"));
                g.gnatives = table;
            }
            None => g.notes.push("GNatives not found inside ProcessEvent (native hooks disabled)".into()),
        }
        // `or dword [rip+X], 2` is 7 bytes with the displacement at +2.
        let pat = memory::parse_pattern(SIG_RUNTIME_UC_FLAGS).expect("built-in signature parses");
        match memory::scan_range(g.process_event, 0x400, &pat)
            .and_then(|instr| unsafe { memory::read::<i32>(instr + 2) }.map(|d| (instr as isize + 7 + d as isize) as usize))
        {
            Some(addr) => {
                crate::log::line("resolve", &format!("GRuntimeUCFlags at 0x{addr:x}"));
                g.runtime_uc_flags = addr;
            }
            None => g.notes.push("GRuntimeUCFlags not found (optional-parm defaults in hooked natives skipped)".into()),
        }
    }
}

/// `ProcessInternal` is whatever `Func` the overwhelming majority of
/// `UFunction` objects share (every script function binds to it). Needs the
/// Engine package loaded so the sample is large.
pub fn resolve_process_internal() -> Option<usize> {
    let function_class = find_class("Core.Function")?;
    let mut counts: HashMap<usize, usize> = HashMap::new();
    for obj in objects() {
        if object_class(obj) != Some(function_class) {
            continue;
        }
        if let Some(f) = unsafe { memory::read::<usize>(obj + UFUNCTION_FUNC) } {
            if f != 0 {
                *counts.entry(f).or_default() += 1;
            }
        }
    }
    let (addr, n) = counts.into_iter().max_by_key(|(_, n)| *n)?;
    let module = memory::main_module()?;
    if addr < module.base || addr >= module.base + module.size {
        return None;
    }
    crate::log::line("resolve", &format!("ProcessInternal at 0x{addr:x} ({n} script functions bind to it)"));
    if let Some(g) = GLOBALS.lock().unwrap().as_mut() {
        g.process_internal = addr;
    }
    Some(addr)
}

/// Poll until `GNames[0] == "None"` and GObjects has a readable object.
fn wait_until_ready(g: &mut Globals) {
    let deadline = Instant::now() + READY_TIMEOUT;
    let mut announced = false;
    loop {
        if !g.gnames_ok {
            g.gnames_ok = name_at_in(g.gnames, 0, 0).as_deref() == Some("None");
        }
        if g.gnames_ok && !g.gobjects_ok && g.gobjects != 0 {
            g.gobjects_ok = first_valid_object(g.gobjects).is_some();
        }
        if g.gnames_ok && (g.gobjects_ok || g.gobjects == 0) {
            TRUSTED.store(true, Ordering::Relaxed);
            crate::log::line("resolve", "GNames/GObjects live");
            return;
        }
        if Instant::now() >= deadline {
            if !g.gnames_ok {
                g.notes.push(match name_at_in(g.gnames, 0, 0) {
                    Some(n) => format!("GNames[0] = `{n}`, expected `None` (mask/TLS/entry layout suspect)"),
                    None => "GNames never became readable (TLS key table / signature suspect)".into(),
                });
            }
            if !g.gobjects_ok {
                g.notes.push("GObjects has no readable object".into());
            }
            return;
        }
        if !announced {
            announced = true;
            crate::log::line("resolve", "waiting for the game to populate GNames/GObjects");
        }
        store(g);
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Block until `*GEngine` exists (the Engine package and its classes are
/// loaded by then) or `timeout` passes. The launcher injects within the
/// first second, when only `Core` is loaded; scripts that look up classes
/// or names at load time need this.
pub fn wait_for_engine(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let mut announced = false;
    while engine().is_none() {
        if Instant::now() >= deadline {
            crate::log::line("resolve", "GEngine still null; running scripts anyway");
            return false;
        }
        if !announced {
            announced = true;
            crate::log::line("resolve", "waiting for GEngine (Engine package load)");
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    crate::log::line(
        "resolve",
        &format!(
            "GEngine live; objects={} names={}",
            object_count().unwrap_or(0),
            name_count().unwrap_or(0)
        ),
    );
    if resolve_process_internal().is_none() {
        if let Some(g) = GLOBALS.lock().unwrap().as_mut() {
            g.notes.push("ProcessInternal unresolved (script-function hooks disabled)".into());
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Engine allocator
// ---------------------------------------------------------------------------

/// `GMalloc->Realloc(ptr, size, 8)`; `ptr == 0` allocates. `None` when the
/// allocator is unresolved or the engine returned null.
pub fn engine_realloc(ptr: usize, size: usize) -> Option<usize> {
    let slot = globals().gmalloc;
    let gmalloc: usize = unsafe { memory::read(slot)? };
    let vtable: usize = unsafe { memory::read(gmalloc)? };
    let f: usize = unsafe { memory::read(vtable + GMALLOC_REALLOC)? };
    #[cfg(windows)]
    {
        if f == 0 {
            return None;
        }
        // SAFETY: FMalloc::Realloc(this, ptr, size, align), thiscall == system on x64.
        let realloc: extern "system" fn(usize, usize, u32, u32) -> usize = unsafe { std::mem::transmute(f) };
        let p = realloc(gmalloc, ptr, size as u32, 8);
        (p != 0).then_some(p)
    }
    #[cfg(not(windows))]
    {
        let _ = (ptr, size, f);
        None
    }
}

/// `GMalloc->Free(ptr)`. No-op when unresolved (leaks instead of crashing).
pub fn engine_free(ptr: usize) {
    if ptr == 0 {
        return;
    }
    let slot = globals().gmalloc;
    let Some(gmalloc) = (unsafe { memory::read::<usize>(slot) }) else { return };
    let Some(vtable) = (unsafe { memory::read::<usize>(gmalloc) }) else { return };
    let Some(f) = (unsafe { memory::read::<usize>(vtable + GMALLOC_FREE) }) else { return };
    #[cfg(windows)]
    {
        if f != 0 {
            let free: extern "system" fn(usize, usize) = unsafe { std::mem::transmute(f) };
            free(gmalloc, ptr);
        }
    }
    #[cfg(not(windows))]
    let _ = f;
}

fn first_valid_object(gobjects: usize) -> Option<usize> {
    let (_, count) = cheat_array(gobjects)?;
    (0..count.min(256))
        .filter_map(|i| decrypt_entry(gobjects, i))
        .find(|&obj| object_class(obj).and_then(object_name).is_some())
}

// ---------------------------------------------------------------------------
// Encrypted arrays, names, objects
// ---------------------------------------------------------------------------

/// `(Data, Count)` of a `TAntiCheatArray`.
fn cheat_array(array: usize) -> Option<(usize, usize)> {
    let data: usize = unsafe { memory::read(array)? };
    let count: i32 = unsafe { memory::read(array + CHEAT_ARRAY_COUNT)? };
    if data == 0 || !(0..=16_000_000).contains(&count) {
        return None;
    }
    Some((data, count as usize))
}

/// Decrypted, non-null entry `i` of a `TAntiCheatArray`.
fn decrypt_entry(array: usize, i: usize) -> Option<usize> {
    let (data, count) = cheat_array(array)?;
    if i >= count {
        return None;
    }
    let raw: usize = unsafe { memory::read(data + i * 8)? };
    if raw == 0 {
        return None;
    }
    let ptr = raw ^ crate::tls::xor_key(i)?;
    // A wrong key (table not filled yet, bad mask) yields garbage; never
    // dereference it. Until the arrays validated, also probe the page.
    if !(0x10000..0x0000_8000_0000_0000).contains(&ptr) {
        return None;
    }
    if !TRUSTED.load(Ordering::Relaxed) && !memory::is_readable(ptr, 0x60) {
        return None;
    }
    Some(ptr)
}

/// Set once `GNames[0] == "None"` and an object decoded: from then on
/// decrypted pointers are trusted without a page probe (hot paths).
static TRUSTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn object_count() -> Option<usize> {
    cheat_array(globals().gobjects).map(|(_, n)| n)
}

pub fn name_count() -> Option<usize> {
    cheat_array(globals().gnames).map(|(_, n)| n)
}

/// Live object at GObjects slot `i`.
pub fn object_at(i: usize) -> Option<usize> {
    decrypt_entry(globals().gobjects, i)
}

/// Resolve an `FName` (`Index`, `Number`) to its string.
pub fn name_at(index: i32, number: i32) -> Option<String> {
    name_at_in(globals().gnames, index, number)
}

fn name_at_in(gnames: usize, index: i32, number: i32) -> Option<String> {
    if index < 0 || gnames == 0 {
        return None;
    }
    let entry = decrypt_entry(gnames, index as usize)?;
    let flags: u32 = unsafe { memory::read(entry + FNAME_ENTRY_INDEX)? };
    let mut name = if flags & 1 != 0 {
        memory::read_wstr(entry + FNAME_ENTRY_NAME, 1024)?
    } else {
        memory::read_cstr(entry + FNAME_ENTRY_NAME, 1024)?
    };
    // UE3 stores the display suffix +1 (`NAME_INTERNAL_TO_EXTERNAL`):
    // "PlayerController_0" has Number == 1.
    if number > 0 {
        name.push('_');
        name.push_str(&(number - 1).to_string());
    }
    Some(name)
}

/// GNames index of an existing name (linear scan, cached).
pub fn find_name(name: &str) -> Option<i32> {
    static CACHE: Mutex<Option<HashMap<String, i32>>> = Mutex::new(None);
    // FNames compare case-insensitively; the table keeps whatever casing
    // registered first ("Playertick" is a real possibility).
    let key = name.to_ascii_lowercase();
    if let Some(hit) = CACHE.lock().unwrap().get_or_insert_default().get(&key) {
        return Some(*hit);
    }
    let gnames = globals().gnames;
    let (_, count) = cheat_array(gnames)?;
    let found = (0..count)
        .find(|&i| name_at_in(gnames, i as i32, 0).is_some_and(|n| n.eq_ignore_ascii_case(name)))?;
    CACHE.lock().unwrap().get_or_insert_default().insert(key, found as i32);
    Some(found as i32)
}

pub fn object_outer(obj: usize) -> Option<usize> {
    if obj == 0 {
        return None;
    }
    unsafe { memory::read::<usize>(obj + UOBJECT_OUTER) }.filter(|&p| p != 0)
}

pub fn object_class(obj: usize) -> Option<usize> {
    if obj == 0 {
        return None;
    }
    unsafe { memory::read::<usize>(obj + UOBJECT_CLASS) }.filter(|&p| p != 0)
}

pub fn object_fname(obj: usize) -> Option<(i32, i32)> {
    if obj == 0 {
        return None;
    }
    unsafe { Some((memory::read(obj + UOBJECT_NAME)?, memory::read(obj + UOBJECT_NAME + 4)?)) }
}

pub fn object_name(obj: usize) -> Option<String> {
    let (i, n) = object_fname(obj)?;
    name_at(i, n)
}

/// `Outer.Outer.Name` (what the SDK calls the path name).
pub fn object_path(obj: usize) -> Option<String> {
    let mut parts = vec![object_name(obj)?];
    let mut cur = object_outer(obj);
    while let Some(o) = cur {
        if parts.len() > 32 {
            break;
        }
        parts.push(object_name(o)?);
        cur = object_outer(o);
    }
    parts.reverse();
    Some(parts.join("."))
}

/// `"Class TgGame.TgGame"` — SDK `GetFullName`.
pub fn object_full_name(obj: usize) -> Option<String> {
    let class = object_name(object_class(obj)?)?;
    Some(format!("{class} {}", object_path(obj)?))
}

/// Walk `Class → SuperField` looking for `class_ptr`.
pub fn is_a(obj: usize, class_ptr: usize) -> bool {
    if obj == 0 || class_ptr == 0 {
        return false;
    }
    super_chain(object_class(obj)).any(|c| c == class_ptr)
}

/// Same, by `"Package.Class"` path — no object scan needed.
pub fn is_a_path(obj: usize, path: &str) -> bool {
    super_chain(object_class(obj)).any(|c| object_path(c).as_deref() == Some(path))
}

fn super_chain(class: Option<usize>) -> impl Iterator<Item = usize> {
    let mut cur = class.filter(|&c| c != 0);
    let mut guard = 0;
    std::iter::from_fn(move || {
        let c = cur?;
        guard += 1;
        if guard > 64 {
            return None;
        }
        cur = unsafe { memory::read::<usize>(c + USTRUCT_SUPER) }.filter(|&p| p != 0);
        Some(c)
    })
}

/// Iterate every live object (decrypted, non-null).
pub fn objects() -> impl Iterator<Item = usize> {
    let gobjects = globals().gobjects;
    let count = cheat_array(gobjects).map(|(_, n)| n).unwrap_or(0);
    (0..count).filter_map(move |i| decrypt_entry(gobjects, i))
}

/// First object whose `GetFullName()` is `full` (e.g. `"Class TgGame.TgGame"`).
pub fn find_object(full: &str) -> Option<usize> {
    let (class_name, path) = full.split_once(' ')?;
    objects().find(|&o| {
        object_class(o).and_then(object_name).as_deref() == Some(class_name)
            && object_path(o).as_deref() == Some(path)
    })
}

/// `UClass*` for `"TgGame.TgGame"`, cached (class objects live forever).
pub fn find_class(path: &str) -> Option<usize> {
    static CACHE: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);
    if let Some(hit) = CACHE.lock().unwrap().get_or_insert_default().get(path) {
        return Some(*hit);
    }
    let found = find_object(&format!("Class {path}"))?;
    CACHE
        .lock()
        .unwrap()
        .get_or_insert_default()
        .insert(path.to_string(), found);
    Some(found)
}

/// `*GEngine`, when the engine exists.
pub fn engine() -> Option<usize> {
    unsafe { memory::read::<usize>(globals().engine_slot) }.filter(|&p| p != 0)
}

// ---------------------------------------------------------------------------
// Property reflection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKind {
    Int,
    Float,
    Byte,
    Bool,
    Name,
    Str,
    Object,
    Struct,
    Array,
    Map,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prop {
    pub offset: usize,
    pub kind: PropKind,
    /// `UBoolProperty.BitMask`.
    pub bitmask: u32,
    /// `UStructProperty.Struct` / `UObjectProperty.PropertyClass` / `UArrayProperty.Inner`.
    pub ext: usize,
    pub elem_size: usize,
}

/// Script-facing value. Struct properties other than Vector/Rotator come
/// back as their address so scripts can fall back to `memory.*`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Object(usize),
    Vector(f32, f32, f32),
    Rotator(i32, i32, i32),
    /// Any other struct: live view (`kind` = `UScriptStruct*`).
    Struct { addr: usize, kind: usize },
    /// `TArray` with its element property (offset 0 within an element).
    Array { addr: usize, inner: Prop },
    /// `TMap` with key/value properties (offsets relative to a pair).
    Map { addr: usize, key: Prop, value: Prop, elem_size: usize },
    Addr(usize),
}

/// Class name of a property object (`"IntProperty"`, `"MapProperty"`, ...).
pub fn prop_class_name(prop: usize) -> Option<String> {
    object_class(prop).and_then(object_name)
}

fn kind_of(prop: usize) -> PropKind {
    match object_class(prop).and_then(object_name).as_deref() {
        Some("IntProperty") => PropKind::Int,
        Some("FloatProperty") => PropKind::Float,
        Some("ByteProperty") => PropKind::Byte,
        Some("BoolProperty") => PropKind::Bool,
        Some("NameProperty") => PropKind::Name,
        Some("StrProperty") => PropKind::Str,
        Some("ObjectProperty" | "ClassProperty" | "ComponentProperty") => PropKind::Object,
        Some("StructProperty") => PropKind::Struct,
        Some("ArrayProperty") => PropKind::Array,
        Some("MapProperty") => PropKind::Map,
        _ => PropKind::Other,
    }
}

fn prop_at(field: usize) -> Option<Prop> {
    if field == 0 {
        return None;
    }
    let kind = kind_of(field);
    if kind == PropKind::Other {
        return None;
    }
    let offset: i32 = unsafe { memory::read(field + UPROP_OFFSET)? };
    let elem_size: i32 = unsafe { memory::read(field + UPROP_ELEMENT_SIZE)? };
    // Maps keep the UMapProperty itself so Key (+0x98) and Value (+0xA0)
    // can both be reached later.
    let ext: usize = if kind == PropKind::Map { field } else { unsafe { memory::read(field + UPROP_EXT)? } };
    Some(Prop {
        offset: offset.max(0) as usize,
        kind,
        bitmask: ext as u32,
        ext,
        elem_size: elem_size.max(0) as usize,
    })
}

/// `Children` linked list of a `UStruct`.
fn children(structure: usize) -> impl Iterator<Item = usize> {
    let mut cur = if structure == 0 {
        None
    } else {
        unsafe { memory::read::<usize>(structure + USTRUCT_CHILDREN) }.filter(|&p| p != 0)
    };
    let mut guard = 0;
    std::iter::from_fn(move || {
        let f = cur?;
        guard += 1;
        if guard > 8192 {
            return None;
        }
        cur = unsafe { memory::read::<usize>(f + UFIELD_NEXT) }.filter(|&p| p != 0);
        Some(f)
    })
}

/// The `UProperty` object for `name` declared directly on `structure`.
pub fn find_property_object_in(structure: usize, name: &str) -> Option<usize> {
    children(structure).find(|&f| object_name(f).as_deref() == Some(name) && prop_class_name(f).is_some_and(|c| c.ends_with("Property")))
}

/// The `UProperty` object for `name` on `class` or a superclass (uncached).
pub fn find_property_object(class: usize, name: &str) -> Option<usize> {
    super_chain(Some(class))
        .flat_map(children)
        .find(|&f| object_name(f).as_deref() == Some(name) && prop_class_name(f).is_some_and(|c| c.ends_with("Property")))
}

/// Property `name` on `class` or any superclass, cached per `(class, name)`.
pub fn find_property(class: usize, name: &str) -> Option<Prop> {
    if class == 0 {
        return None;
    }
    type Cache = HashMap<(usize, String), Option<Prop>>;
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    let key = (class, name.to_string());
    if let Some(hit) = CACHE.lock().unwrap().get_or_insert_default().get(&key) {
        return *hit;
    }
    let found = super_chain(Some(class))
        .flat_map(children)
        .find(|&f| object_name(f).as_deref() == Some(name))
        .and_then(prop_at);
    CACHE.lock().unwrap().get_or_insert_default().insert(key, found);
    found
}

fn struct_name(prop: &Prop) -> Option<String> {
    object_name(prop.ext)
}

/// `UStruct.PropertySize` of a `UScriptStruct`.
pub fn struct_size(ustruct: usize) -> usize {
    if ustruct == 0 {
        return 0;
    }
    unsafe { memory::read::<u32>(ustruct + USTRUCT_PROPERTY_SIZE) }.unwrap_or(0) as usize
}

/// Property names declared on a struct/class and its supers, in order.
pub fn field_names(structure: usize) -> Vec<String> {
    super_chain(Some(structure))
        .flat_map(children)
        .filter(|&f| kind_of(f) != PropKind::Other)
        .filter_map(object_name)
        .collect()
}

fn align_up(v: usize, a: usize) -> usize {
    if a == 0 { v } else { v.div_ceil(a) * a }
}

fn prop_align(p: &Prop) -> usize {
    match p.kind {
        PropKind::Object | PropKind::Str | PropKind::Array | PropKind::Map => 8,
        PropKind::Byte | PropKind::Bool => 1,
        PropKind::Struct => 4.min(p.elem_size.max(1)),
        _ => 4,
    }
    .max(1)
}

/// A synthetic property of a given kind (for native maps, whose pair types
/// reflection does not record). `struct:Pkg.Name` resolves the ScriptStruct.
pub fn synthetic_prop(kind: &str) -> Option<Prop> {
    let (k, size, ext) = match kind.to_ascii_lowercase().as_str() {
        "int" => (PropKind::Int, 4, 0),
        "float" => (PropKind::Float, 4, 0),
        "byte" => (PropKind::Byte, 1, 0),
        "name" => (PropKind::Name, 8, 0),
        "str" | "string" => (PropKind::Str, 16, 0),
        "object" => (PropKind::Object, 8, 0),
        s if s.starts_with("struct:") => {
            let us = find_object(&format!("ScriptStruct {}", &kind[7..]))?;
            (PropKind::Struct, struct_size(us), us)
        }
        _ => return None,
    };
    Some(Prop { offset: 0, kind: k, bitmask: 0, ext, elem_size: size })
}

/// A `Map` value over the `TMap` at `addr` with explicit pair types.
pub fn map_with(addr: usize, key: Prop, value: Prop) -> Value {
    let (value_off, elem_size) = map_layout(&key, &value);
    Value::Map {
        addr,
        key: Prop { offset: 0, ..key },
        value: Prop { offset: value_off, ..value },
        elem_size,
    }
}

/// Layout of a `TMap` pair: `(value offset, element size)`.
fn map_layout(key: &Prop, value: &Prop) -> (usize, usize) {
    let value_off = align_up(key.elem_size, prop_align(value));
    let pair_align = prop_align(key).max(prop_align(value));
    let pair_size = align_up(value_off + value.elem_size, pair_align);
    // + HashNextId, HashIndex
    (value_off, align_up(pair_size + 8, pair_align.max(4)))
}

fn map_value(prop: &Prop) -> Option<Value> {
    let key_field: usize = unsafe { memory::read(prop.ext + UMAP_KEY) }.unwrap_or(0);
    let value_field: usize = unsafe { memory::read(prop.ext + UMAP_VALUE) }.unwrap_or(0);
    let (key, value) = match (prop_at(key_field), prop_at(value_field)) {
        (Some(k), Some(v)) => (k, v),
        _ => {
            static ONCE: Mutex<Option<std::collections::HashSet<usize>>> = Mutex::new(None);
            if ONCE.lock().unwrap().get_or_insert_default().insert(prop.ext) {
                crate::log::line(
                    "unreal",
                    &format!(
                        "map {}: key=0x{key_field:x} ({}) value=0x{value_field:x} ({}); layout unresolved",
                        object_name(prop.ext).unwrap_or_default(),
                        object_class(key_field).and_then(object_name).unwrap_or_else(|| "?".into()),
                        object_class(value_field).and_then(object_name).unwrap_or_else(|| "?".into()),
                    ),
                );
            }
            return None;
        }
    };
    let (value_off, elem_size) = map_layout(&key, &value);
    Some(Value::Map {
        addr: 0,
        key: Prop { offset: 0, ..key },
        value: Prop { offset: value_off, ..value },
        elem_size,
    })
}

// ---------------------------------------------------------------------------
// TArray helpers (Data@0 Num@8 Max@0xC; `inner` has offset 0)
// ---------------------------------------------------------------------------

pub fn array_len(addr: usize) -> usize {
    unsafe { memory::read::<i32>(addr + TARRAY_COUNT) }.unwrap_or(0).max(0) as usize
}

fn array_stride(inner: &Prop) -> usize {
    inner.elem_size.max(1)
}

/// Address of element `i` (0-based), bounds-checked.
pub fn array_elem(addr: usize, inner: &Prop, i: usize) -> Option<usize> {
    if i >= array_len(addr) {
        return None;
    }
    let data: usize = unsafe { memory::read(addr)? };
    (data != 0).then(|| data + i * array_stride(inner))
}

/// Append a zeroed element (growing through the engine allocator) and
/// return its address.
pub fn array_push(addr: usize, inner: &Prop) -> Result<usize, String> {
    let stride = array_stride(inner);
    let data: usize = unsafe { memory::read(addr) }.ok_or("unreadable array")?;
    let num = array_len(addr);
    let max: i32 = unsafe { memory::read(addr + TARRAY_MAX) }.ok_or("unreadable array")?;
    let mut data = data;
    if data == 0 || num >= max.max(0) as usize {
        let new_max = (max.max(0) as usize * 2).max(4).max(num + 1);
        data = engine_realloc(data, new_max * stride).ok_or("engine allocator unavailable")?;
        unsafe {
            memory::write(addr, data);
            memory::write(addr + TARRAY_MAX, new_max as i32);
        }
    }
    let elem = data + num * stride;
    for i in 0..stride {
        unsafe { memory::write(elem + i, 0u8) };
    }
    unsafe { memory::write(addr + TARRAY_COUNT, (num + 1) as i32) };
    Ok(elem)
}

fn release_element(elem: usize, inner: &Prop) {
    if inner.kind == PropKind::Str {
        if let Some(data) = unsafe { memory::read::<usize>(elem) } {
            engine_free(data);
        }
    }
}

/// Remove element `i` (0-based), shifting the tail down.
pub fn array_remove(addr: usize, inner: &Prop, i: usize) -> Result<(), String> {
    let num = array_len(addr);
    let stride = array_stride(inner);
    let elem = array_elem(addr, inner, i).ok_or("index out of range")?;
    release_element(elem, inner);
    copy_bytes(elem + stride, elem, (num - 1 - i) * stride);
    unsafe { memory::write(addr + TARRAY_COUNT, (num - 1) as i32) };
    Ok(())
}

pub fn array_clear(addr: usize, inner: &Prop) {
    for i in 0..array_len(addr) {
        if let Some(elem) = array_elem(addr, inner, i) {
            release_element(elem, inner);
        }
    }
    unsafe { memory::write(addr + TARRAY_COUNT, 0i32) };
}

/// Address of the value slot for `key`, when the key exists.
pub fn map_find_slot(addr: usize, key: &Prop, value: &Prop, elem_size: usize, wanted: &Value) -> Option<usize> {
    let elements: usize = unsafe { memory::read(addr + TMAP_ELEMENTS) }?;
    let count = unsafe { memory::read::<i32>(addr + TMAP_COUNT) }.unwrap_or(0).max(0) as usize;
    if elements == 0 || count > 100_000 {
        return None;
    }
    let indirect: usize = unsafe { memory::read(addr + TMAP_INDIRECT_BITS) }.unwrap_or(0);
    let bits = if indirect != 0 { indirect } else { addr + TMAP_INLINE_BITS };
    for i in 0..count {
        let word: u32 = unsafe { memory::read(bits + (i / 32) * 4) }.unwrap_or(0);
        if word & (1 << (i % 32)) == 0 {
            continue;
        }
        let elem = elements + i * elem_size;
        let same = match (read_prop(elem, key), wanted) {
            (Some(Value::Str(a)), Value::Str(b)) => a.eq_ignore_ascii_case(b),
            (Some(a), b) => &a == b,
            _ => false,
        };
        if same {
            return Some(elem + value.offset);
        }
    }
    None
}

/// Live `(key, value)` pairs of a `TMap` (allocated slots only).
pub fn map_entries(addr: usize, key: &Prop, value: &Prop, elem_size: usize) -> Vec<(Value, Value)> {
    let mut out = Vec::new();
    let Some(elements) = (unsafe { memory::read::<usize>(addr + TMAP_ELEMENTS) }) else { return out };
    let count = unsafe { memory::read::<i32>(addr + TMAP_COUNT) }.unwrap_or(0).max(0) as usize;
    if elements == 0 || count == 0 || count > 100_000 {
        return out;
    }
    let indirect: usize = unsafe { memory::read(addr + TMAP_INDIRECT_BITS) }.unwrap_or(0);
    let bits = if indirect != 0 { indirect } else { addr + TMAP_INLINE_BITS };
    for i in 0..count {
        let word: u32 = unsafe { memory::read(bits + (i / 32) * 4) }.unwrap_or(0);
        if word & (1 << (i % 32)) == 0 {
            continue;
        }
        let elem = elements + i * elem_size;
        let k = read_prop(elem, key).unwrap_or(Value::Nil);
        let v = read_prop(elem, value).unwrap_or(Value::Nil);
        out.push((k, v));
    }
    out
}

/// Read `prop` from the instance at `base`.
pub fn read_prop(base: usize, prop: &Prop) -> Option<Value> {
    if base == 0 || !memory::is_readable(base + prop.offset, prop.elem_size.max(1)) {
        return None;
    }
    let addr = base + prop.offset;
    unsafe {
        Some(match prop.kind {
            PropKind::Int => Value::Int(memory::read::<i32>(addr)? as i64),
            PropKind::Float => Value::Float(memory::read::<f32>(addr)? as f64),
            PropKind::Byte => Value::Int(memory::read::<u8>(addr)? as i64),
            PropKind::Bool => Value::Bool(memory::read::<u32>(addr)? & prop.bitmask != 0),
            PropKind::Name => {
                let (i, n): (i32, i32) = (memory::read(addr)?, memory::read(addr + 4)?);
                Value::Str(name_at(i, n)?)
            }
            PropKind::Str => read_fstring(addr).map(Value::Str).unwrap_or(Value::Nil),
            PropKind::Object => match memory::read::<usize>(addr)? {
                0 => Value::Nil,
                p => Value::Object(p),
            },
            PropKind::Struct => match struct_name(prop).as_deref() {
                Some("Vector") => Value::Vector(
                    memory::read(addr)?,
                    memory::read(addr + 4)?,
                    memory::read(addr + 8)?,
                ),
                Some("Rotator") => Value::Rotator(
                    memory::read(addr)?,
                    memory::read(addr + 4)?,
                    memory::read(addr + 8)?,
                ),
                _ => Value::Struct { addr, kind: prop.ext },
            },
            PropKind::Array => match prop_at(prop.ext) {
                Some(inner) => Value::Array { addr, inner: Prop { offset: 0, ..inner } },
                None => Value::Addr(addr),
            },
            PropKind::Map => match map_value(prop) {
                Some(Value::Map { key, value, elem_size, .. }) => Value::Map { addr, key, value, elem_size },
                _ => Value::Addr(addr),
            },
            PropKind::Other => Value::Addr(addr),
        })
    }
}

/// Write `value` into `prop` on the instance at `base`. Type-checked against
/// the reflected kind; `FString` writes must fit the existing buffer.
pub fn write_prop(base: usize, prop: &Prop, value: &Value) -> Result<(), String> {
    if base == 0 {
        return Err("null object".into());
    }
    let addr = base + prop.offset;
    let ok = unsafe {
        match (prop.kind, value) {
            (PropKind::Int, Value::Int(v)) => memory::write(addr, *v as i32),
            (PropKind::Int, Value::Float(v)) => memory::write(addr, *v as i32),
            (PropKind::Float, Value::Float(v)) => memory::write(addr, *v as f32),
            (PropKind::Float, Value::Int(v)) => memory::write(addr, *v as f32),
            (PropKind::Byte, Value::Int(v)) => memory::write(addr, *v as u8),
            (PropKind::Bool, Value::Bool(v)) => {
                let word: u32 = memory::read(addr).ok_or("unreadable")?;
                let word = if *v { word | prop.bitmask } else { word & !prop.bitmask };
                memory::write(addr, word)
            }
            (PropKind::Name, Value::Str(s)) => {
                let idx = find_name(s).ok_or_else(|| format!("unknown FName `{s}`"))?;
                memory::write(addr, idx) && memory::write(addr + 4, 0i32)
            }
            (PropKind::Str, Value::Str(s)) => return write_fstring(addr, s),
            (PropKind::Object, Value::Object(p)) => memory::write(addr, *p),
            (PropKind::Object, Value::Nil) => memory::write(addr, 0usize),
            (PropKind::Struct, Value::Vector(x, y, z))
                if struct_name(prop).as_deref() == Some("Vector") =>
            {
                memory::write(addr, *x) && memory::write(addr + 4, *y) && memory::write(addr + 8, *z)
            }
            (PropKind::Struct, Value::Rotator(p, y, r))
                if struct_name(prop).as_deref() == Some("Rotator") =>
            {
                memory::write(addr, *p) && memory::write(addr + 4, *y) && memory::write(addr + 8, *r)
            }
            // Whole-struct copy from another live struct of the same type.
            (PropKind::Struct, Value::Struct { addr: src, kind }) if *kind == prop.ext => {
                let n = struct_size(*kind).max(prop.elem_size);
                copy_bytes(*src, addr, n);
                true
            }
            (kind, v) => return Err(format!("cannot assign {v:?} to a {kind:?} property")),
        }
    };
    if ok {
        Ok(())
    } else {
        Err("write failed".into())
    }
}

/// `FString` = `TArray<wchar_t>` (NUL included in `Count`).
pub fn read_fstring(addr: usize) -> Option<String> {
    let data: usize = unsafe { memory::read(addr)? };
    let count: i32 = unsafe { memory::read(addr + TARRAY_COUNT)? };
    if data == 0 || count <= 0 {
        return Some(String::new());
    }
    memory::read_wstr(data, count.min(4096) as usize)
}

/// `FString` write. Reuses the buffer when it fits, otherwise allocates a
/// new one with the engine allocator and frees the old one.
pub fn write_fstring(addr: usize, s: &str) -> Result<(), String> {
    let data: usize = unsafe { memory::read(addr) }.ok_or("unreadable FString")?;
    let max: i32 = unsafe { memory::read(addr + TARRAY_MAX) }.ok_or("unreadable FString")?;
    let wide: Vec<u16> = s.encode_utf16().collect();
    let needed = wide.len() + 1;
    let (target, new_max) = if data != 0 && needed <= max.max(0) as usize {
        (data, max)
    } else {
        let fresh = engine_realloc(0, needed * 2)
            .ok_or_else(|| format!("`{s}` does not fit (Max={max}) and the engine allocator is unresolved"))?;
        (fresh, needed as i32)
    };
    unsafe {
        for (i, w) in wide.iter().enumerate() {
            memory::write(target + i * 2, *w);
        }
        memory::write(target + wide.len() * 2, 0u16);
        if target != data {
            memory::write(addr, target);
            memory::write(addr + TARRAY_MAX, new_max);
            engine_free(data);
        }
        memory::write(addr + TARRAY_COUNT, needed as i32);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// UFunctions: params, lookup, calls
// ---------------------------------------------------------------------------

/// One `CPF_Parm` of a `UFunction`.
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub prop: Prop,
    pub flags: u64,
}

impl Param {
    pub fn is_optional(&self) -> bool {
        self.flags & CPF_OPTIONAL_PARM != 0
    }
    pub fn is_out(&self) -> bool {
        self.flags & CPF_OUT_PARM != 0
    }
    pub fn is_return(&self) -> bool {
        self.flags & CPF_RETURN_PARM != 0
    }
}

/// All params of a `UFunction` in declaration order (return value included,
/// flagged), cached per function.
pub fn function_params(func: usize) -> Vec<Param> {
    if func == 0 {
        return Vec::new();
    }
    type Cache = HashMap<usize, Vec<Param>>;
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    if let Some(hit) = CACHE.lock().unwrap().get_or_insert_default().get(&func) {
        return hit.clone();
    }
    let params: Vec<Param> = children(func)
        .filter_map(|f| {
            let flags: u64 = unsafe { memory::read(f + UPROP_FLAGS)? };
            if flags & CPF_PARM == 0 {
                return None;
            }
            Some(Param {
                name: object_name(f)?,
                prop: prop_at(f)?,
                flags,
            })
        })
        .collect();
    CACHE
        .lock()
        .unwrap()
        .get_or_insert_default()
        .insert(func, params.clone());
    params
}

/// `UFunction*` named `name` on `class` or a superclass, cached.
pub fn find_function(class: usize, name: &str) -> Option<usize> {
    if class == 0 {
        return None;
    }
    type Cache = HashMap<(usize, String), Option<usize>>;
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    let key = (class, name.to_string());
    if let Some(hit) = CACHE.lock().unwrap().get_or_insert_default().get(&key) {
        return *hit;
    }
    let found = super_chain(Some(class)).flat_map(children).find(|&f| {
        object_name(f).as_deref() == Some(name)
            && object_class(f).and_then(object_name).as_deref() == Some("Function")
    });
    CACHE.lock().unwrap().get_or_insert_default().insert(key, found);
    found
}

/// Native ⇔ `Func` is not `ProcessInternal` (falls back to `FUNC_Native`
/// until `ProcessInternal` is known).
pub fn function_is_native(func: usize) -> bool {
    let pi = globals().process_internal;
    let f: usize = unsafe { memory::read(func + UFUNCTION_FUNC) }.unwrap_or(0);
    if pi != 0 && f != 0 {
        return f != pi;
    }
    unsafe { memory::read::<u32>(func + UFUNCTION_FLAGS) }.is_some_and(|f| f & FUNC_NATIVE != 0)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn function_is_script(func: usize) -> bool {
    let pi = globals().process_internal;
    pi != 0 && unsafe { memory::read::<usize>(func + UFUNCTION_FUNC) } == Some(pi)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn copy_bytes(src: usize, dst: usize, n: usize) {
    if src == 0 || dst == 0 {
        return;
    }
    for i in 0..n {
        if let Some(b) = unsafe { memory::read::<u8>(src + i) } {
            unsafe { memory::write(dst + i, b) };
        }
    }
}

/// Copy the (POD) return value of `func` from one frame/slot to another.
/// Strings and arrays are skipped: they need assignment, not a byte copy.
#[cfg_attr(not(windows), allow(dead_code))]
fn copy_return(func: usize, from: usize, from_is_frame: bool, to: usize) {
    let Some(p) = function_params(func).into_iter().find(|p| p.is_return()) else { return };
    if matches!(p.prop.kind, PropKind::Str | PropKind::Array | PropKind::Other) {
        return;
    }
    let src = if from_is_frame { from + p.prop.offset } else { from };
    copy_bytes(src, to, p.prop.elem_size.max(1));
}

fn parms_frame_size(func: usize, params: &[Param]) -> usize {
    let declared: u32 = unsafe { memory::read(func + USTRUCT_PROPERTY_SIZE) }.unwrap_or(0);
    let computed = params
        .iter()
        .map(|p| p.prop.offset + p.prop.elem_size.max(1))
        .max()
        .unwrap_or(0);
    (declared as usize).max(computed)
}

/// Call `func` on `obj` with positional `args` through the original
/// `ProcessEvent` (our own hooks are bypassed). Returns the return value
/// (if any) followed by every `out` param, as `(name, value)` pairs.
///
/// `FString` args are engine-allocated when `GMalloc` resolved (so a callee
/// may reassign them); otherwise they point at a script-owned buffer that a
/// reassigning callee would hand to the engine allocator. Returned/out
/// strings are freed after being read.
pub fn call_function(obj: usize, func: usize, args: &[Value]) -> Result<Vec<(String, Value)>, String> {
    if obj == 0 || func == 0 {
        return Err("null object or function".into());
    }
    let params = function_params(func);
    let size = parms_frame_size(func, &params);
    // u64-backed so every param is naturally aligned.
    let mut frame = vec![0u64; size.div_ceil(8).max(1)];
    let base = frame.as_mut_ptr() as usize;
    let mut buffers: Vec<Vec<u16>> = Vec::new();

    for (i, p) in params.iter().filter(|p| !p.is_return()).enumerate() {
        let arg = args.get(i).unwrap_or(&Value::Nil);
        match (p.prop.kind, arg) {
            (_, Value::Nil) => {}
            (PropKind::Str, Value::Str(s)) if !p.is_out() => {
                let mut wide: Vec<u16> = s.encode_utf16().collect();
                wide.push(0);
                let addr = base + p.prop.offset;
                let data = match engine_realloc(0, wide.len() * 2) {
                    Some(p) => {
                        for (i, w) in wide.iter().enumerate() {
                            unsafe { memory::write(p + i * 2, *w) };
                        }
                        p
                    }
                    None => {
                        buffers.push(wide.clone());
                        buffers.last().unwrap().as_ptr() as usize
                    }
                };
                unsafe {
                    memory::write(addr, data);
                    memory::write(addr + TARRAY_COUNT, wide.len() as i32);
                    memory::write(addr + TARRAY_MAX, wide.len() as i32);
                }
            }
            (PropKind::Str, _) => {}
            _ => write_prop(base, &p.prop, arg).map_err(|e| format!("arg {} `{}`: {e}", i + 1, p.name))?,
        }
    }

    // Args not passed at all (as opposed to an explicit nil) count as
    // omitted, so a hooked native applies its script defaults to them.
    let omitted: Vec<bool> = params
        .iter()
        .filter(|p| !p.is_return())
        .enumerate()
        .map(|(i, _)| i >= args.len())
        .collect();
    #[cfg(windows)]
    let handled = native::call_with_frame(func, obj, base, &omitted);
    #[cfg(not(windows))]
    let _ = &omitted;
    #[cfg(not(windows))]
    let handled = false;
    if !handled {
        call_original(obj, func, base)?;
    }

    let mut out: Vec<(String, Value)> = params
        .iter()
        .filter(|p| p.is_return())
        .map(|p| (p.name.clone(), read_prop(base, &p.prop).unwrap_or(Value::Nil)))
        .collect();
    out.extend(
        params
            .iter()
            .filter(|p| p.is_out() && !p.is_return())
            .map(|p| (p.name.clone(), read_prop(base, &p.prop).unwrap_or(Value::Nil))),
    );
    // Every FString in the frame is engine-owned now (args we allocated,
    // returns/outs the engine allocated): give them back.
    for p in &params {
        if p.prop.kind == PropKind::Str {
            if let Some(data) = unsafe { memory::read::<usize>(base + p.prop.offset) } {
                if data != 0 && !buffers.iter().any(|b| b.as_ptr() as usize == data) {
                    engine_free(data);
                }
            }
        }
    }
    drop(buffers);
    Ok(out)
}

fn call_original(obj: usize, func: usize, parms: usize) -> Result<(), String> {
    #[cfg(windows)]
    {
        hook::call_original(obj, func, parms)
    }
    #[cfg(not(windows))]
    {
        let _ = (obj, func, parms);
        Err("host build: no game".into())
    }
}

// ---------------------------------------------------------------------------
// Event subscriptions (before / after ProcessEvent)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Runs before the original; may edit params or cancel the call.
    Before,
    /// Runs after the original; sees out params and the return value.
    After,
}

#[derive(Debug, Clone)]
struct Sub {
    /// Mod that subscribed (`""` for the prelude).
    owner: String,
    phase: Phase,
    /// `"TgGame.TgGame"` from a qualified spec.
    class_path: Option<String>,
    func_name: String,
    /// The exact `UFunction` a qualified spec binds to (resolved at subscribe
    /// time, or on first name hit once the class is loaded). With it, the
    /// hook fires exactly when that function body runs, wherever in a
    /// `Super` chain that is. Without it, matching is by name (IsA-gated when
    /// a class was given) and skips inner `Super` frames.
    func_ptr: Option<usize>,
    /// Resolved lazily: the first time a function with this name fires.
    fname_index: Option<i32>,
}

static SUBS: Mutex<Vec<(u64, Sub)>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// `"PostLogin"` or `"TgGame.TgGame.PostLogin"`. Returns the subscription id.
pub fn subscribe(spec: &str, phase: Phase, owner: &str) -> u64 {
    let (class_path, func_name) = match spec.rsplit_once('.') {
        Some((c, f)) => (Some(c.to_string()), f.to_string()),
        None => (None, spec.to_string()),
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let ready = globals().gnames_ok;
    let fname_index = if ready { find_name(&func_name) } else { None };
    let func_ptr = match class_path.as_deref() {
        Some(cp) if ready => find_class(cp).and_then(|c| find_function(c, &func_name)),
        _ => None,
    };
    crate::log::line(
        "events",
        &format!(
            "subscribe #{id} {phase:?} {spec} ({})",
            match (func_ptr, fname_index) {
                (Some(f), _) => format!("exact 0x{f:x}"),
                (None, Some(i)) => format!("by name, fname={i}"),
                (None, None) => "by name, lazy".into(),
            }
        ),
    );
    SUBS.lock().unwrap().push((
        id,
        Sub {
            owner: owner.to_string(),
            phase,
            class_path: class_path.clone(),
            func_name: func_name.clone(),
            func_ptr,
            fname_index,
        },
    ));
    // Natives only reach a hook through their own `Func` pointer, which needs
    // the concrete UFunction: resolve it when the spec names the class.
    if let (Some(cp), Some(func)) = (class_path.as_deref(), func_ptr) {
        if function_is_native(func) {
            match hook_native(func) {
                Ok(()) => crate::log::line("events", &format!("native hook on {cp}.{func_name}")),
                Err(e) if e.contains("GPropAddr") => {
                    crate::log::line("events", &format!("native hook on {cp}.{func_name} deferred: {e}"));
                    PENDING_NATIVE_HOOKS.lock().unwrap().push(func);
                }
                Err(e) => crate::log::line("events", &format!("cannot hook native {cp}.{func_name}: {e}")),
            }
        }
    }
    id
}

/// Any subscription (either phase) for this function? Cheap gate for hooks.
#[cfg_attr(not(windows), allow(dead_code))]
fn any_sub_for(function: usize) -> bool {
    let subs = SUBS.lock().unwrap();
    if subs.is_empty() {
        return false;
    }
    let Some((index, _)) = object_fname(function) else { return false };
    let mut fired_name: Option<Option<String>> = None;
    subs.iter().any(|(_, s)| match (s.func_ptr, s.fname_index) {
        (Some(f), _) => f == function,
        (None, Some(i)) => i == index,
        (None, None) => fired_name
            .get_or_insert_with(|| object_name(function))
            .as_deref()
            .is_some_and(|n| n.eq_ignore_ascii_case(&s.func_name)),
    })
}

pub fn unsubscribe(id: u64) {
    SUBS.lock().unwrap().retain(|(i, _)| *i != id);
}

pub fn clear_subscriptions() -> Vec<u64> {
    SUBS.lock().unwrap().drain(..).map(|(id, _)| id).collect()
}

/// Drop every subscription owned by `owner`; returns their ids.
pub fn clear_subscriptions_of(owner: &str) -> Vec<u64> {
    let mut subs = SUBS.lock().unwrap();
    let gone: Vec<u64> = subs
        .iter()
        .filter(|(_, s)| s.owner.eq_ignore_ascii_case(owner))
        .map(|(id, _)| *id)
        .collect();
    subs.retain(|(id, _)| !gone.contains(id));
    gone
}

/// Owner of a subscription, if it still exists.
pub fn subscription_owner(id: u64) -> Option<String> {
    SUBS.lock().unwrap().iter().find(|(i, _)| *i == id).map(|(_, s)| s.owner.clone())
}

pub fn subscriptions() -> Vec<(u64, String)> {
    SUBS.lock()
        .unwrap()
        .iter()
        .map(|(id, s)| {
            let spec = match &s.class_path {
                Some(c) => format!("{c}.{}", s.func_name),
                None => s.func_name.clone(),
            };
            let tag = match s.phase {
                Phase::Before => "before ",
                Phase::After => "",
            };
            let owner = if s.owner.is_empty() { String::new() } else { format!("[{}] ", s.owner) };
            (*id, format!("{owner}{tag}{spec}"))
        })
        .collect()
}

/// One fired event, handed to the Lua side. `parms` is the live params
/// frame (read `function_params(function)` for its layout).
#[derive(Debug, Clone)]
pub struct Event {
    pub sub_id: u64,
    pub phase: Phase,
    pub object: usize,
    pub function: usize,
    pub parms: usize,
}

/// Returns `false` to cancel the original call (`Before` phase only).
static DISPATCHER: OnceLock<fn(Event) -> bool> = OnceLock::new();

pub fn set_dispatcher(f: fn(Event) -> bool) {
    let _ = DISPATCHER.set(f);
}

/// `inner`: this frame is a `Super.Func()` call inside an outer frame of the
/// same name on the same object; name-matched subscriptions fire once per
/// logical call (at the outermost frame), exact-function ones wherever that
/// function body runs.
fn matching_subs(object: usize, function: usize, phase: Phase, inner: bool) -> Vec<u64> {
    let Some((index, _)) = object_fname(function) else { return Vec::new() };
    let mut subs = SUBS.lock().unwrap();
    if subs.is_empty() {
        return Vec::new();
    }
    let mut fired_name: Option<Option<String>> = None;
    let mut due = Vec::new();
    for (id, s) in subs.iter_mut() {
        if s.phase != phase {
            continue;
        }
        if let Some(f) = s.func_ptr {
            if f == function {
                due.push(*id);
            }
            continue;
        }
        let same_name = match s.fname_index {
            Some(i) => i == index,
            None => {
                let name = fired_name.get_or_insert_with(|| object_name(function));
                let hit = name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&s.func_name));
                if hit {
                    s.fname_index = Some(index);
                }
                hit
            }
        };
        if !same_name {
            continue;
        }
        let hit = match s.class_path.as_deref() {
            None => !inner,
            Some(cp) => {
                // The class may have loaded since subscribe: bind exactly now.
                match find_class(cp).and_then(|c| find_function(c, &s.func_name)) {
                    Some(f) => {
                        s.func_ptr = Some(f);
                        f == function
                    }
                    None => !inner && is_a_path(object, cp),
                }
            }
        };
        if hit {
            due.push(*id);
        }
    }
    due
}

/// Hook entry, before the original. Returns `false` to skip it.
pub fn on_process_event_before(object: usize, function: usize, parms: usize, inner: bool) -> bool {
    let Some(dispatch) = DISPATCHER.get() else { return true };
    let mut proceed = true;
    for sub_id in matching_subs(object, function, Phase::Before, inner) {
        if !dispatch(Event {
            sub_id,
            phase: Phase::Before,
            object,
            function,
            parms,
        }) {
            proceed = false;
        }
    }
    proceed
}

/// Hook entry, after the original returned.
pub fn on_process_event_after(object: usize, function: usize, parms: usize, inner: bool) {
    let Some(dispatch) = DISPATCHER.get() else { return };
    for sub_id in matching_subs(object, function, Phase::After, inner) {
        dispatch(Event {
            sub_id,
            phase: Phase::After,
            object,
            function,
            parms,
        });
    }
}

// ---------------------------------------------------------------------------
// Game-thread queue
// ---------------------------------------------------------------------------

type Job = Box<dyn FnOnce() + Send>;
static QUEUE: Mutex<Vec<Job>> = Mutex::new(Vec::new());
/// Thread that fired the first `ProcessEvent`; `0` until then.
static GAME_THREAD: AtomicU32 = AtomicU32::new(0);

fn current_thread_id() -> u32 {
    #[cfg(windows)]
    unsafe {
        GetCurrentThreadId()
    }
    #[cfg(not(windows))]
    0
}

pub fn game_thread_id() -> Option<u32> {
    match GAME_THREAD.load(Ordering::Relaxed) {
        0 => None,
        t => Some(t),
    }
}

pub fn on_game_thread() -> bool {
    game_thread_id() == Some(current_thread_id())
}

/// Run `job` on the game thread at the next event (never drops it; if no
/// event ever fires the job stays queued).
pub fn queue(job: Job) {
    QUEUE.lock().unwrap().push(job);
}

/// Natives whose hook waits for `GPropAddr` (they have `out` params).
static PENDING_NATIVE_HOOKS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Find `GPropAddr` once a live frame header exists: execute one
/// `EX_NativeParm` for a known property against a scratch frame and look
/// for the global that now holds `Locals + Offset`. Candidates are the
/// `mov [rip+X], reg` targets inside `execNativeParm` itself.
#[cfg(windows)]
fn resolve_gpropaddr() -> Option<usize> {
    let g = globals();
    if g.gpropaddr != 0 {
        return Some(g.gpropaddr);
    }
    if g.gnatives == 0 || !construct::has_frame_header() {
        return None;
    }
    let exec_addr: usize = unsafe { memory::read(g.gnatives + EX_NATIVE_PARM as usize * 8) }.filter(|&p| p != 0)?;
    let actor = find_class("Engine.Actor")?;
    let field = find_property_object(actor, "Location")?;
    let prop = prop_at(field)?;
    // Candidate globals: rip-relative 64-bit stores in the exec's first 0x100 bytes.
    let module = memory::main_module()?;
    let mut candidates = Vec::new();
    for off in 0..0x100usize {
        let a = exec_addr + off;
        let b0: u8 = unsafe { memory::read(a) }?;
        let b1: u8 = unsafe { memory::read(a + 1) }?;
        let b2: u8 = unsafe { memory::read(a + 2) }?;
        if (b0 == 0x48 || b0 == 0x4C) && b1 == 0x89 && b2 & 0xC7 == 0x05 {
            let disp: i32 = unsafe { memory::read(a + 3) }?;
            let target = (a as isize + 7 + disp as isize) as usize;
            if target >= module.base && target < module.base + module.size && !candidates.contains(&target) {
                candidates.push(target);
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let before: Vec<usize> = candidates.iter().map(|&c| unsafe { memory::read::<usize>(c) }.unwrap_or(0)).collect();
    // Scratch frame + `[EX_NativeParm][field*]` stream, Result = scratch slot.
    let mut locals = [0u8; 0x400];
    let lp = locals.as_mut_ptr() as usize;
    let mut code = vec![EX_NATIVE_PARM];
    code.extend_from_slice(&field.to_le_bytes());
    let mut frame = [0u8; FRAME_SIZE];
    let fp = frame.as_mut_ptr() as usize;
    construct::write_frame_header(fp);
    let mut scratch = [0u8; 64];
    unsafe {
        memory::write(fp + FRAME_NODE, actor);
        memory::write(fp + FRAME_OBJECT, actor);
        memory::write(fp + FRAME_CODE, code.as_ptr() as usize + 1);
        memory::write(fp + FRAME_LOCALS, lp);
        memory::write(fp + FRAME_PREVIOUS, 0usize);
        memory::write(fp + FRAME_OUT_PARMS, 0usize);
    }
    let exec: extern "system" fn(usize, usize, usize) = unsafe { std::mem::transmute(exec_addr) };
    exec(actor, fp, scratch.as_mut_ptr() as usize);
    let expected = lp + prop.offset;
    let found = candidates
        .iter()
        .zip(before.iter())
        .find(|(&c, &b)| unsafe { memory::read::<usize>(c) } == Some(expected) && b != expected)
        .map(|(&c, _)| c);
    match found {
        Some(addr) => {
            crate::log::line("resolve", &format!("GPropAddr at 0x{addr:x} (probe over {} candidates)", candidates.len()));
            if let Some(g) = GLOBALS.lock().unwrap().as_mut() {
                g.gpropaddr = addr;
            }
            Some(addr)
        }
        None => {
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| crate::log::line("resolve", &format!("GPropAddr probe found nothing among {} candidates; out-param natives stay unhookable", candidates.len())));
            None
        }
    }
}

/// Game-thread housekeeping: resolve `GPropAddr`, then hook natives that
/// were waiting on it.
#[cfg(windows)]
fn service_pending_hooks() {
    if globals().gpropaddr == 0 {
        if PENDING_NATIVE_HOOKS.lock().unwrap().is_empty() {
            return;
        }
        if resolve_gpropaddr().is_none() {
            return;
        }
    }
    let pending: Vec<usize> = std::mem::take(&mut *PENDING_NATIVE_HOOKS.lock().unwrap());
    for func in pending {
        match hook_native(func) {
            Ok(()) => crate::log::line("events", &format!("native hook on {} (deferred)", object_full_name(func).unwrap_or_default())),
            Err(e) => crate::log::line("events", &format!("cannot hook native {}: {e}", object_full_name(func).unwrap_or_default())),
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn drain_queue() {
    let tid = current_thread_id();
    if GAME_THREAD
        .compare_exchange(0, tid, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        crate::log::line("events", &format!("game thread = t{tid}"));
    }
    if !on_game_thread() {
        return;
    }
    crate::input::maintain(false);
    #[cfg(windows)]
    service_pending_hooks();
    let jobs: Vec<Job> = std::mem::take(&mut *QUEUE.lock().unwrap());
    for job in jobs {
        job();
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
}

// ---------------------------------------------------------------------------
// ProcessEvent hook (Windows only)
// ---------------------------------------------------------------------------

/// Construct a new object of `class` inside `outer` (optionally named)
/// through the engine's own `new` operator: a six-expression bytecode
/// stream executed by `GNatives[EX_New]`, which does everything
/// `StaticConstructObject` does (config load, constructor, defaults).
/// Needs a live frame header captured from the first hooked call.
pub fn construct_object(class: usize, outer: usize, name: Option<&str>) -> Result<usize, String> {
    #[cfg(windows)]
    {
        construct::new_object(class, outer, name)
    }
    #[cfg(not(windows))]
    {
        let _ = (class, outer, name);
        Err("host build".into())
    }
}

/// Hooked native functions, as full names (for `tempest.status()`).
pub fn native_hooks() -> Vec<String> {
    #[cfg(windows)]
    {
        native::hooked().into_iter().filter_map(object_full_name).collect()
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// Swap a native `UFunction.Func` for one of our thunks (idempotent).
pub fn hook_native(func: usize) -> Result<(), String> {
    #[cfg(windows)]
    {
        native::hook(func)
    }
    #[cfg(not(windows))]
    {
        let _ = func;
        Err("host build".into())
    }
}

/// Detour `ProcessInternal` so script→script calls reach hooks too.
pub fn install_script_hook() {
    #[cfg(windows)]
    {
        let target = globals().process_internal;
        if target == 0 {
            return;
        }
        match script_hook::install(target) {
            Ok(()) => {
                if let Some(g) = GLOBALS.lock().unwrap().as_mut() {
                    g.script_hook_installed = true;
                }
                crate::log::line("hook", &format!("ProcessInternal detour live at 0x{target:x}"));
            }
            Err(e) => crate::log::line("hook", &format!("ProcessInternal detour failed: {e}")),
        }
    }
}

/// Detour `ProcessEvent`, once. Needs `resolve()` to have found it.
pub fn install_hook() {
    #[cfg(windows)]
    {
        let target = globals().process_event;
        if target == 0 {
            crate::log::line("hook", "ProcessEvent unresolved; events disabled");
            return;
        }
        match hook::install(target) {
            Ok(()) => {
                if let Some(g) = GLOBALS.lock().unwrap().as_mut() {
                    g.hook_installed = true;
                }
                crate::log::line("hook", &format!("ProcessEvent detour live at 0x{target:x}"));
            }
            Err(e) => crate::log::line("hook", &format!("detour failed: {e}")),
        }
    }
}

#[cfg(windows)]
mod hook {
    use std::ffi::c_void;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::OnceLock;

    type ProcessEventFn =
        extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void);

    static HOOK: OnceLock<retour::GenericDetour<ProcessEventFn>> = OnceLock::new();

    extern "system" fn hooked(
        this: *mut c_void,
        func: *mut c_void,
        parms: *mut c_void,
        result: *mut c_void,
    ) {
        let Some(h) = HOOK.get() else { return };
        let (o, f, p) = (this as usize, func as usize, parms as usize);
        let _ = catch_unwind(super::drain_queue);
        // Script functions dispatch from the ProcessInternal detour and
        // hooked natives from their thunk; dispatching here too would double up.
        if super::function_is_script(f) || super::native::is_hooked(f) {
            h.call(this, func, parms, result);
            return;
        }
        // A script error is already a Lua error; a Rust panic must not take
        // the game thread down with it (and never cancels the call).
        let proceed = catch_unwind(AssertUnwindSafe(|| super::on_process_event_before(o, f, p, false)))
            .unwrap_or(true);
        if !proceed {
            return;
        }
        h.call(this, func, parms, result);
        let _ = catch_unwind(AssertUnwindSafe(|| super::on_process_event_after(o, f, p, false)));
    }

    /// Call the original (un-hooked) `ProcessEvent`; falls back to the raw
    /// resolved address when the detour is not installed.
    pub fn call_original(obj: usize, func: usize, parms: usize) -> Result<(), String> {
        let (o, f, p) = (obj as *mut c_void, func as *mut c_void, parms as *mut c_void);
        if let Some(h) = HOOK.get() {
            h.call(o, f, p, std::ptr::null_mut());
            return Ok(());
        }
        let raw = super::globals().process_event;
        if raw == 0 {
            return Err("ProcessEvent unresolved".into());
        }
        // SAFETY: resolved UObject::ProcessEvent; same ABI as the detour type.
        let original: ProcessEventFn = unsafe { std::mem::transmute(raw) };
        original(o, f, p, std::ptr::null_mut());
        Ok(())
    }

    pub fn install(target: usize) -> Result<(), String> {
        if HOOK.get().is_some() {
            return Ok(());
        }
        // SAFETY: `target` is UObject::ProcessEvent resolved from the proven
        // call-site signature; both sides use the same 4-pointer ABI.
        let original: ProcessEventFn = unsafe { std::mem::transmute(target) };
        let detour = unsafe { retour::GenericDetour::new(original, hooked) }
            .map_err(|e| format!("create: {e}"))?;
        unsafe { detour.enable() }.map_err(|e| format!("enable: {e}"))?;
        HOOK.set(detour).map_err(|_| "already installed".to_string())
    }
}

// ---------------------------------------------------------------------------
// ProcessInternal hook: every script function execution (Windows only)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod script_hook {
    use super::layout::*;
    use crate::memory;
    use std::ffi::c_void;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::OnceLock;

    type ProcessInternalFn = extern "system" fn(*mut c_void, *mut c_void, *mut c_void);

    static HOOK: OnceLock<retour::GenericDetour<ProcessInternalFn>> = OnceLock::new();

    extern "system" fn hooked(this: *mut c_void, stack: *mut c_void, result: *mut c_void) {
        let Some(h) = HOOK.get() else { return };
        let (o, s, r) = (this as usize, stack as usize, result as usize);
        super::construct::capture_frame_header(s);
        let func = unsafe { memory::read::<usize>(s + FRAME_NODE) }.unwrap_or(0);
        let locals = unsafe { memory::read::<usize>(s + FRAME_LOCALS) }.unwrap_or(0);
        if func == 0 || locals == 0 || !super::any_sub_for(func) {
            h.call(this, stack, result);
            return;
        }
        // `Super.Func()` runs as a nested frame whose caller is a same-named
        // function on the same object.
        let inner = unsafe { memory::read::<usize>(s + FRAME_PREVIOUS) }
            .filter(|&prev| prev != 0)
            .is_some_and(|prev| {
                let prev_object = unsafe { memory::read::<usize>(prev + FRAME_OBJECT) };
                let prev_name = unsafe { memory::read::<usize>(prev + FRAME_NODE) }
                    .and_then(super::object_fname)
                    .map(|(i, _)| i);
                prev_object == Some(o) && prev_name == super::object_fname(func).map(|(i, _)| i)
            });
        let proceed = catch_unwind(AssertUnwindSafe(|| super::on_process_event_before(o, func, locals, inner)))
            .unwrap_or(true);
        if !proceed {
            // The caller still expects its return slot filled.
            super::copy_return(func, locals, true, r);
            return;
        }
        h.call(this, stack, result);
        let _ = catch_unwind(AssertUnwindSafe(|| super::on_process_event_after(o, func, locals, inner)));
    }

    pub fn install(target: usize) -> Result<(), String> {
        if HOOK.get().is_some() {
            return Ok(());
        }
        // SAFETY: `target` is UObject::ProcessInternal (the Func every script
        // function binds to); same (this, FFrame*, void*) ABI as the thunk.
        let original: ProcessInternalFn = unsafe { std::mem::transmute(target) };
        let detour = unsafe { retour::GenericDetour::new(original, hooked) }
            .map_err(|e| format!("create: {e}"))?;
        unsafe { detour.enable() }.map_err(|e| format!("enable: {e}"))?;
        HOOK.set(detour).map_err(|_| "already installed".to_string())
    }
}

// ---------------------------------------------------------------------------
// Native hooks: swap `UFunction.Func` for a thunk (Windows only)
// ---------------------------------------------------------------------------
//
// A native called from UnrealScript receives the *caller's* frame with
// `Code` pointing at its argument expressions and evaluates them itself.
// The thunk evaluates them first (exactly as the native would, through
// `GNatives`), runs the handlers against that frame, then re-enters the
// native with a frame whose `Code` is the function's own `EX_NativeParm`
// script, so the native reads the (possibly edited) values back out of the
// frame — the same mechanism `ProcessEvent` uses to run natives.

#[cfg(windows)]
mod native {
    use super::layout::*;
    use super::{function_params, globals, parms_frame_size, PropKind};
    use crate::memory;
    use std::ffi::c_void;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    pub const SLOTS: usize = 64;
    type NativeFn = extern "system" fn(*mut c_void, *mut c_void, *mut c_void);
    /// `GNatives[op](Context, Stack&, Result)`.
    type ExecFn = extern "system" fn(usize, usize, usize);

    struct Slot {
        func: AtomicUsize,
        original: AtomicUsize,
    }
    static TABLE: [Slot; SLOTS] = [const {
        Slot {
            func: AtomicUsize::new(0),
            original: AtomicUsize::new(0),
        }
    }; SLOTS];
    static INSTALL: Mutex<()> = Mutex::new(());

    extern "system" fn thunk<const N: usize>(this: *mut c_void, stack: *mut c_void, result: *mut c_void) {
        let func = TABLE[N].func.load(Ordering::Acquire);
        let original: NativeFn = unsafe { std::mem::transmute(TABLE[N].original.load(Ordering::Acquire)) };
        let (o, s, r) = (this as usize, stack as usize, result as usize);
        if func == 0 || s == 0 || !super::any_sub_for(func) {
            original(this, stack, result);
            return;
        }
        if catch_unwind(AssertUnwindSafe(|| dispatch(func, original, o, s, r))).is_err() {
            crate::log::line("hook", "native thunk panicked; call skipped");
        }
    }

    fn dispatch(func: usize, original: NativeFn, this: usize, stack: usize, result: usize) {
        let params = function_params(func);
        let size = parms_frame_size(func, &params);
        let mut frame = vec![0u64; size.div_ceil(8).max(1)];
        let buf = frame.as_mut_ptr() as usize;
        let g = globals();
        let gnatives = g.gnatives;
        let context = unsafe { memory::read::<usize>(stack + FRAME_OBJECT) }.unwrap_or(this);

        // Evaluate the caller's argument expressions into our frame. An
        // omitted optional arg is `EX_EmptyParmValue`, which raises
        // RUC_SkippedOptionalParm; remember that per param so the default
        // prologue below applies defaults to exactly those.
        let mut omitted = vec![false; params.len()];
        // Caller l-value address of each `out` param (0 = none / unknown).
        let mut out_addrs = vec![0usize; params.len()];
        for (i, p) in params.iter().enumerate().filter(|(_, p)| !p.is_return()) {
            let Some(code) = (unsafe { memory::read::<usize>(stack + FRAME_CODE) }) else { break };
            let Some(op) = (unsafe { memory::read::<u8>(code) }) else { break };
            unsafe { memory::write(stack + FRAME_CODE, code + 1) };
            let exec = unsafe { memory::read::<usize>(gnatives + op as usize * 8) }.unwrap_or(0);
            if exec == 0 {
                crate::log::line("hook", &format!("native thunk: no exec for opcode 0x{op:02x}"));
                break;
            }
            set_skipped_flag(g.runtime_uc_flags, false);
            if g.gpropaddr != 0 {
                unsafe { memory::write(g.gpropaddr, 0usize) };
            }
            let exec: ExecFn = unsafe { std::mem::transmute(exec) };
            exec(context, stack, buf + p.prop.offset);
            omitted[i] = skipped_flag(g.runtime_uc_flags);
            if p.is_out() && g.gpropaddr != 0 {
                out_addrs[i] = unsafe { memory::read::<usize>(g.gpropaddr) }.unwrap_or(0);
            }
        }
        set_skipped_flag(g.runtime_uc_flags, false);
        // P_FINISH
        if let Some(code) = unsafe { memory::read::<usize>(stack + FRAME_CODE) } {
            if unsafe { memory::read::<u8>(code) } == Some(EX_END_FUNCTION_PARMS) {
                unsafe { memory::write(stack + FRAME_CODE, code + 1) };
            }
        }

        run(func, original, this, stack, buf, result, &params, &omitted, &out_addrs);
    }

    fn skipped_flag(addr: usize) -> bool {
        unsafe { memory::read::<u32>(addr) }.is_some_and(|f| f & RUC_SKIPPED_OPTIONAL_PARM != 0)
    }

    fn set_skipped_flag(addr: usize, on: bool) {
        if let Some(f) = unsafe { memory::read::<u32>(addr) } {
            let v = if on { f | RUC_SKIPPED_OPTIONAL_PARM } else { f & !RUC_SKIPPED_OPTIONAL_PARM };
            unsafe { memory::write(addr, v) };
        }
    }

    /// Handlers + the native itself, against an already-filled frame `buf`.
    /// `omitted[i]`: the caller did not pass param `i` (defaults apply).
    #[allow(clippy::too_many_arguments)]
    fn run(func: usize, original: NativeFn, this: usize, stack: usize, buf: usize, result: usize, params: &[super::Param], omitted: &[bool], out_addrs: &[usize]) {
        if !super::on_process_event_before(this, func, buf, false) {
            super::copy_return(func, buf, true, result);
            return;
        }

        // Re-enter the native against our frame via its NativeParm script.
        let mut fake = [0u8; FRAME_SIZE];
        let fake_ptr = fake.as_mut_ptr() as usize;
        if stack != 0 {
            super::copy_bytes(stack, fake_ptr, FRAME_NODE); // FOutputDevice header
        }
        let script = unsafe { memory::read::<usize>(func + UFUNCTION_SCRIPT_DATA) }.unwrap_or(0);
        // Out params: an FOutParmRec chain into our frame, as ProcessEvent
        // builds one, so execNativeParm resolves them to our slots; the
        // results are copied back to the caller's variables afterwards.
        let out_params: Vec<(usize, &super::Param)> = params
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_out() && !p.is_return())
            .collect();
        let mut recs = vec![0u8; OUTPARM_SIZE * out_params.len().max(1)];
        let recs_ptr = recs.as_mut_ptr() as usize;
        let mut head = 0usize;
        for (n, (_, p)) in out_params.iter().enumerate().rev() {
            let rec = recs_ptr + n * OUTPARM_SIZE;
            let field = super::find_property_object_in(func, &p.name).unwrap_or(0);
            unsafe {
                memory::write(rec + OUTPARM_PROPERTY, field);
                memory::write(rec + OUTPARM_ADDR, buf + p.prop.offset);
                memory::write(rec + OUTPARM_NEXT, head);
            }
            head = rec;
        }
        unsafe {
            memory::write(fake_ptr + FRAME_NODE, func);
            memory::write(fake_ptr + FRAME_OBJECT, this);
            memory::write(fake_ptr + FRAME_CODE, script);
            memory::write(fake_ptr + FRAME_LOCALS, buf);
            memory::write(fake_ptr + FRAME_PREVIOUS, stack);
            memory::write(fake_ptr + FRAME_OUT_PARMS, head);
        }
        // Same prologue as ProcessEvent: the script of a function with
        // optional params starts with one default-value opcode per optional
        // param, which fills the frame slot only when the skipped flag is set
        // and always advances Code to the NativeParm stream behind it.
        let g = globals();
        let flags: u32 = unsafe { memory::read(func + UFUNCTION_FLAGS) }.unwrap_or(0);
        if flags & FUNC_HAS_OPTIONAL_PARMS != 0 && g.gnatives != 0 {
            for (i, p) in params.iter().enumerate() {
                if p.is_return() || p.flags & CPF_PARM == 0 {
                    break;
                }
                if !p.is_optional() {
                    continue;
                }
                set_skipped_flag(g.runtime_uc_flags, omitted.get(i).copied().unwrap_or(false));
                let Some(code) = (unsafe { memory::read::<usize>(fake_ptr + FRAME_CODE) }) else { break };
                let Some(op) = (unsafe { memory::read::<u8>(code) }) else { break };
                unsafe { memory::write(fake_ptr + FRAME_CODE, code + 1) };
                let exec = unsafe { memory::read::<usize>(g.gnatives + op as usize * 8) }.unwrap_or(0);
                if exec == 0 {
                    break;
                }
                // ProcessEvent passes Result = Parms + Offset here: the default
                // expression (e.g. EX_True) writes straight into the param slot.
                let exec: ExecFn = unsafe { std::mem::transmute(exec) };
                exec(this, fake_ptr, buf + p.prop.offset);
            }
            set_skipped_flag(g.runtime_uc_flags, false);
        }
        original(this as *mut c_void, fake_ptr as *mut c_void, result as *mut c_void);

        // Out params back to the caller's variables (POD kinds only; a
        // string/array out param would need assignment semantics).
        for (i, p) in out_params {
            let dst = out_addrs.get(i).copied().unwrap_or(0);
            if dst != 0 && !matches!(p.prop.kind, PropKind::Str | PropKind::Array | PropKind::Map | PropKind::Other) {
                super::copy_bytes(buf + p.prop.offset, dst, p.prop.elem_size.max(1));
            }
        }
        // Natives write the return value straight to `result`; mirror it so
        // after-handlers can read `params.ReturnValue`.
        if let Some(p) = params.iter().find(|p| p.is_return()) {
            if result != buf + p.prop.offset {
                super::copy_bytes(result, buf + p.prop.offset, p.prop.elem_size.max(1));
            }
        }
        super::on_process_event_after(this, func, buf, false);
    }

    /// Lua-initiated call of a hooked native: `ProcessEvent` would run the
    /// native through its own path and skip the thunk, so run handlers and
    /// the native here against the caller-built frame instead.
    pub fn call_with_frame(func: usize, this: usize, buf: usize, omitted_inputs: &[bool]) -> bool {
        let Some(slot) = TABLE.iter().find(|s| s.func.load(Ordering::Acquire) == func) else { return false };
        let original: NativeFn = unsafe { std::mem::transmute(slot.original.load(Ordering::Acquire)) };
        let params = function_params(func);
        let result = params.iter().find(|p| p.is_return()).map(|p| buf + p.prop.offset).unwrap_or(0);
        // `omitted_inputs` is indexed over non-return params; spread it over all.
        let mut omitted = vec![false; params.len()];
        let mut j = 0;
        for (i, p) in params.iter().enumerate() {
            if !p.is_return() {
                omitted[i] = omitted_inputs.get(j).copied().unwrap_or(true);
                j += 1;
            }
        }
        let out_addrs = vec![0usize; params.len()];
        run(func, original, this, 0, buf, result, &params, &omitted, &out_addrs);
        true
    }

    static THUNKS: [NativeFn; SLOTS] = [thunk::<0>, thunk::<1>, thunk::<2>, thunk::<3>, thunk::<4>, thunk::<5>, thunk::<6>, thunk::<7>, thunk::<8>, thunk::<9>, thunk::<10>, thunk::<11>, thunk::<12>, thunk::<13>, thunk::<14>, thunk::<15>, thunk::<16>, thunk::<17>, thunk::<18>, thunk::<19>, thunk::<20>, thunk::<21>, thunk::<22>, thunk::<23>, thunk::<24>, thunk::<25>, thunk::<26>, thunk::<27>, thunk::<28>, thunk::<29>, thunk::<30>, thunk::<31>, thunk::<32>, thunk::<33>, thunk::<34>, thunk::<35>, thunk::<36>, thunk::<37>, thunk::<38>, thunk::<39>, thunk::<40>, thunk::<41>, thunk::<42>, thunk::<43>, thunk::<44>, thunk::<45>, thunk::<46>, thunk::<47>, thunk::<48>, thunk::<49>, thunk::<50>, thunk::<51>, thunk::<52>, thunk::<53>, thunk::<54>, thunk::<55>, thunk::<56>, thunk::<57>, thunk::<58>, thunk::<59>, thunk::<60>, thunk::<61>, thunk::<62>, thunk::<63>];

    pub fn is_hooked(func: usize) -> bool {
        func != 0 && TABLE.iter().any(|s| s.func.load(Ordering::Acquire) == func)
    }

    pub fn hooked() -> Vec<usize> {
        TABLE
            .iter()
            .map(|s| s.func.load(Ordering::Acquire))
            .filter(|&f| f != 0)
            .collect()
    }

    pub fn hook(func: usize) -> Result<(), String> {
        let _guard = INSTALL.lock().unwrap();
        if is_hooked(func) {
            return Ok(());
        }
        let g = globals();
        if g.gnatives == 0 {
            return Err("GNatives unresolved".into());
        }
        let original = unsafe { memory::read::<usize>(func + UFUNCTION_FUNC) }.unwrap_or(0);
        if original == 0 || original == g.process_internal {
            return Err("not a native function".into());
        }
        let script_num: i32 = unsafe { memory::read(func + UFUNCTION_SCRIPT_NUM) }.unwrap_or(0);
        if script_num <= 0 {
            return Err("native has no NativeParm script (cannot re-enter it with a frame)".into());
        }
        if function_params(func).iter().any(|p| p.is_out() && !p.is_return()) && g.gpropaddr == 0 {
            return Err("has `out` params and GPropAddr is not resolved yet (retried once the game thread runs)".into());
        }
        let slot = TABLE
            .iter()
            .position(|s| s.func.load(Ordering::Acquire) == 0)
            .ok_or_else(|| format!("all {SLOTS} native hook slots in use"))?;
        TABLE[slot].original.store(original, Ordering::Release);
        TABLE[slot].func.store(func, Ordering::Release);
        if !unsafe { memory::write(func + UFUNCTION_FUNC, THUNKS[slot] as usize) } {
            TABLE[slot].func.store(0, Ordering::Release);
            return Err("cannot write UFunction.Func".into());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Object construction through `GNatives[EX_New]` (Windows only)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod construct {
    use super::layout::*;
    use super::{globals, object_class, object_name};
    use crate::memory;
    use std::ffi::c_void;
    use std::sync::OnceLock;

    /// First 0x14 bytes of a live `FFrame` (FOutputDevice vtable + flags);
    /// `execNew` may `Logf` through it on error, so it must be real.
    static HEADER: OnceLock<[u8; FRAME_NODE]> = OnceLock::new();

    pub fn has_frame_header() -> bool {
        HEADER.get().is_some()
    }

    /// Copy the captured header into a frame being built.
    pub fn write_frame_header(frame: usize) {
        if let Some(h) = HEADER.get() {
            for (i, b) in h.iter().enumerate() {
                unsafe { memory::write(frame + i, *b) };
            }
        }
    }

    pub fn capture_frame_header(stack: usize) {
        if HEADER.get().is_some() || stack == 0 {
            return;
        }
        let mut h = [0u8; FRAME_NODE];
        for (i, b) in h.iter_mut().enumerate() {
            match unsafe { memory::read::<u8>(stack + i) } {
                Some(v) => *b = v,
                None => return,
            }
        }
        let _ = HEADER.set(h);
    }

    type ExecFn = extern "system" fn(usize, *mut c_void, *mut c_void);

    pub fn new_object(class: usize, outer: usize, name: Option<&str>) -> Result<usize, String> {
        if class == 0 {
            return Err("null class".into());
        }
        if object_class(class).and_then(object_name).as_deref() != Some("Class") {
            return Err("first argument is not a UClass".into());
        }
        let g = globals();
        if g.gnatives == 0 {
            return Err("GNatives unresolved".into());
        }
        let Some(header) = HEADER.get() else {
            return Err("no frame header captured yet (needs one hooked call first)".into());
        };
        let exec = unsafe { memory::read::<usize>(g.gnatives + EX_NEW as usize * 8) }.unwrap_or(0);
        if exec == 0 {
            return Err("GNatives[EX_New] is null".into());
        }

        // new(Outer, Name, Flags) Class, Template  — five sub-expressions.
        let mut code: Vec<u8> = vec![EX_NEW];
        code.push(EX_OBJECT_CONST);
        code.extend_from_slice(&outer.to_le_bytes());
        match name {
            Some(n) if !n.is_empty() => {
                code.push(EX_STRING_CONST);
                code.extend(n.bytes().filter(|&b| b != 0));
                code.push(0);
            }
            _ => code.push(EX_NOTHING),
        }
        code.push(EX_NOTHING); // Flags = 0
        code.push(EX_OBJECT_CONST);
        code.extend_from_slice(&class.to_le_bytes());
        code.push(EX_NOTHING); // Template = none
        code.push(EX_END_FUNCTION_PARMS); // belt and braces; execNew stops after five

        let mut locals = [0u64; 8];
        let mut frame = [0u8; FRAME_SIZE];
        frame[..FRAME_NODE].copy_from_slice(header);
        let fp = frame.as_mut_ptr() as usize;
        let context = if outer != 0 { outer } else { class };
        unsafe {
            memory::write(fp + FRAME_NODE, class);
            memory::write(fp + FRAME_OBJECT, context);
            memory::write(fp + FRAME_CODE, code.as_ptr() as usize + 1); // past EX_New: we call its exec directly
            memory::write(fp + FRAME_LOCALS, locals.as_mut_ptr() as usize);
            memory::write(fp + FRAME_PREVIOUS, 0usize);
            memory::write(fp + FRAME_OUT_PARMS, 0usize);
        }
        let mut result: usize = 0;
        let exec: ExecFn = unsafe { std::mem::transmute(exec) };
        exec(context, fp as *mut c_void, &mut result as *mut usize as *mut c_void);
        if result == 0 {
            return Err("engine returned none (abstract class, Actor subclass, or bad outer?)".into());
        }
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// SDK dump: LuaLS stubs from live reflection
// ---------------------------------------------------------------------------

fn lua_type_of(prop: usize) -> String {
    let Some(p) = prop_at(prop) else { return "any".into() };
    match p.kind {
        PropKind::Int | PropKind::Byte => "integer".into(),
        PropKind::Float => "number".into(),
        PropKind::Bool => "boolean".into(),
        PropKind::Name | PropKind::Str => "string".into(),
        PropKind::Object => object_path(p.ext).unwrap_or_else(|| "Object".into()),
        PropKind::Struct => match object_name(p.ext).as_deref() {
            Some("Vector") => "Vector".into(),
            Some("Rotator") => "Rotator".into(),
            _ => object_path(p.ext).unwrap_or_else(|| "Struct".into()),
        },
        PropKind::Array => {
            let inner: usize = unsafe { memory::read(prop + UPROP_EXT) }.unwrap_or(0);
            format!("Array<{}>", lua_type_of(inner))
        }
        PropKind::Map => "Map".into(),
        PropKind::Other => "any".into(),
    }
}

fn safe_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Emit one `---@class` block for a UClass or UScriptStruct.
fn dump_struct(obj: usize, out: &mut String) -> bool {
    let Some(path) = object_path(obj) else { return false };
    if !path.split('.').all(safe_ident) {
        return false;
    }
    let is_struct = object_class(obj).and_then(object_name).as_deref() == Some("ScriptStruct");
    let base = unsafe { memory::read::<usize>(obj + USTRUCT_SUPER) }
        .filter(|&p| p != 0)
        .and_then(object_path)
        .unwrap_or_else(|| if is_struct { "Struct".into() } else { "Object".into() });
    out.push_str(&format!("---@class {path} : {base}\n"));
    for field in children(obj) {
        let Some(name) = object_name(field) else { continue };
        if !safe_ident(&name) {
            continue;
        }
        let fclass = prop_class_name(field).unwrap_or_default();
        if fclass == "Function" {
            let params = function_params(field);
            let args: Vec<String> = params
                .iter()
                .filter(|p| !p.is_return())
                .map(|p| {
                    let opt = if p.is_optional() { "?" } else { "" };
                    let kind = match p.prop.kind {
                        PropKind::Int | PropKind::Byte => "integer".to_string(),
                        PropKind::Float => "number".to_string(),
                        PropKind::Bool => "boolean".to_string(),
                        PropKind::Name | PropKind::Str => "string".to_string(),
                        PropKind::Object => object_path(p.prop.ext).unwrap_or_else(|| "Object".into()),
                        PropKind::Struct => match object_name(p.prop.ext).as_deref() {
                            Some("Vector") => "Vector".into(),
                            Some("Rotator") => "Rotator".into(),
                            _ => object_path(p.prop.ext).unwrap_or_else(|| "Struct".into()),
                        },
                        _ => "any".to_string(),
                    };
                    format!("{}{opt}: {kind}", p.name)
                })
                .collect();
            let ret = params.iter().find(|p| p.is_return()).map(|p| match p.prop.kind {
                PropKind::Int | PropKind::Byte => "integer".to_string(),
                PropKind::Float => "number".to_string(),
                PropKind::Bool => "boolean".to_string(),
                PropKind::Name | PropKind::Str => "string".to_string(),
                PropKind::Object => object_path(p.prop.ext).unwrap_or_else(|| "Object".into()),
                _ => "any".to_string(),
            });
            let ret = ret.map(|r| format!(": {r}")).unwrap_or_default();
            out.push_str(&format!("---@field {name} fun(self: {path}{}{}){ret}\n", if args.is_empty() { "" } else { ", " }, args.join(", ")));
        } else if fclass.ends_with("Property") {
            out.push_str(&format!("---@field {name} {}\n", lua_type_of(field)));
        }
    }
    out.push('\n');
    true
}

/// Write `<dir>/<Package>.lua` LuaLS stubs for every class and struct.
/// Returns `(classes, files)`.
pub fn dump_sdk(dir: &std::path::Path) -> Result<(usize, usize), String> {
    let class_class = find_class("Core.Class").ok_or("Core.Class not found (engine not ready?)")?;
    let struct_class = find_class("Core.ScriptStruct").ok_or("Core.ScriptStruct not found")?;
    let mut per_pkg: HashMap<String, String> = HashMap::new();
    let mut classes = 0usize;
    for obj in objects() {
        let Some(cls) = object_class(obj) else { continue };
        if cls != class_class && cls != struct_class {
            continue;
        }
        // Package = outermost outer.
        let mut top = obj;
        while let Some(o) = object_outer(top) {
            top = o;
        }
        let Some(pkg) = object_name(top) else { continue };
        if !safe_ident(&pkg) {
            continue;
        }
        let buf = per_pkg.entry(pkg).or_insert_with(|| "---@meta\n-- Generated by tempest.dump_sdk() from live reflection. Do not edit.\n\n".to_string());
        if dump_struct(obj, buf) {
            classes += 1;
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files = 0usize;
    for (pkg, text) in per_pkg {
        let path = dir.join(format!("{pkg}.lua"));
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        files += 1;
    }
    Ok((classes, files))
}
