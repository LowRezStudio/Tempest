//! What scripting costs the game: time and calls per mod and per entry point
//! (hook and draw handlers, key bindings, console commands, queued jobs,
//! address hooks, script loads), and the runtime's own work around them (the
//! event hooks, building and drawing the UI). Off until
//! `tempest.profile(true)`; until then every entry point costs one relaxed
//! atomic load.
//!
//! Times are self times, kept on a per-thread stack of open scopes: a handler
//! whose game call fires another mod's hook does not carry that hook's cost,
//! and the event hook's time excludes the handlers it ran. The game's own work
//! inside a handler's game call stays with the handler: the mod caused it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

static ON: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Where Lua was entered.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Site {
    /// `tempest.on` / `tempest.before` / `tempest.native` subscription id.
    Hook(u64),
    /// `memory.hook` id.
    Address(u64),
    /// `ui.on_draw` handler id.
    Draw(u64),
    /// A kind with a name: key bindings and console commands (prelude), script loads (file).
    Named(&'static str, String),
    Queue,
    Other,
}

impl Site {
    fn kind(&self) -> &'static str {
        match self {
            Site::Hook(_) => "hook",
            Site::Address(_) => "address",
            Site::Draw(_) => "draw",
            Site::Named(k, _) => k,
            Site::Queue => "queue",
            Site::Other => "other",
        }
    }
}

/// The runtime's own work.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Runtime {
    /// `ProcessEvent` detour: the subscription check, the queue drain, delivery.
    EventHook = 0,
    /// `ProcessInternal` detour: every script function the game runs.
    ScriptHook = 1,
    /// Building the UI frame on the game thread (around the draw handlers).
    UiFrame = 2,
    /// Drawing the UI in the swap chain's Present (the render thread).
    UiRender = 3,
}

const RUNTIME_NAMES: [&str; 4] = ["event hook (ProcessEvent)", "script hook (ProcessInternal)", "UI frame (game thread)", "UI drawing (render thread)"];

struct Counter {
    calls: AtomicU64,
    ns: AtomicU64,
    max: AtomicU64,
}

impl Counter {
    const fn new() -> Counter {
        Counter { calls: AtomicU64::new(0), ns: AtomicU64::new(0), max: AtomicU64::new(0) }
    }

    fn reset(&self) {
        self.calls.store(0, Ordering::Relaxed);
        self.ns.store(0, Ordering::Relaxed);
        self.max.store(0, Ordering::Relaxed);
    }
}

static RUNTIME: [Counter; 4] = [Counter::new(), Counter::new(), Counter::new(), Counter::new()];
/// Game-thread runtime time in the frame being built (render-thread drawing aside).
static RUNTIME_FRAME: AtomicU64 = AtomicU64::new(0);

#[derive(Default, Clone)]
pub struct Stat {
    pub calls: u64,
    pub ns: u64,
    pub max: u64,
    pub label: String,
}

#[derive(Default)]
struct ModStat {
    sites: HashMap<Site, Stat>,
    frame: u64,
    worst_frame: u64,
}

struct State {
    started: Instant,
    frames: u64,
    last_frame: Option<Instant>,
    worst_frame: u64,
    runtime_worst_frame: u64,
    mods: HashMap<String, ModStat>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

thread_local! {
    /// Child time of every open scope on this thread, innermost last.
    static OPEN: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

fn begin() -> Instant {
    OPEN.with(|o| o.borrow_mut().push(0));
    Instant::now()
}

/// Self time of the scope opened at `start`; its total goes to the parent's children.
fn end(start: Instant) -> u64 {
    let total = start.elapsed().as_nanos() as u64;
    let child = OPEN.with(|o| {
        let mut o = o.borrow_mut();
        let c = o.pop().unwrap_or(0);
        if let Some(parent) = o.last_mut() {
            *parent += total;
        }
        c
    });
    total.saturating_sub(child)
}

/// Run Lua entry `f` as `owner` at `site`, timed when profiling.
pub fn lua<R>(owner: &str, site: Site, label: impl FnOnce() -> String, f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    let start = begin();
    let r = f();
    let own = end(start);
    if let Some(s) = STATE.lock().unwrap().as_mut() {
        let m = s.mods.entry(owner.to_string()).or_default();
        m.frame += own;
        let st = m.sites.entry(site).or_insert_with(|| Stat { label: label(), ..Stat::default() });
        st.calls += 1;
        st.ns += own;
        st.max = st.max.max(own);
    }
    r
}

/// Run the runtime's own work `f`, timed when profiling.
pub fn runtime<R>(which: Runtime, f: impl FnOnce() -> R) -> R {
    timed(which, true, f)
}

/// More of the same call's work (after the original ran): time, no new call.
pub fn runtime_more<R>(which: Runtime, f: impl FnOnce() -> R) -> R {
    timed(which, false, f)
}

fn timed<R>(which: Runtime, call: bool, f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    let start = begin();
    let r = f();
    let own = end(start);
    let c = &RUNTIME[which as usize];
    if call {
        c.calls.fetch_add(1, Ordering::Relaxed);
    }
    c.ns.fetch_add(own, Ordering::Relaxed);
    c.max.fetch_max(own, Ordering::Relaxed);
    if which != Runtime::UiRender {
        RUNTIME_FRAME.fetch_add(own, Ordering::Relaxed);
    }
    r
}

/// A UI frame was built: close the per-frame totals (game thread, once per Present).
pub fn frame() {
    if !enabled() {
        return;
    }
    let now = Instant::now();
    let runtime = RUNTIME_FRAME.swap(0, Ordering::Relaxed);
    if let Some(s) = STATE.lock().unwrap().as_mut() {
        if let Some(last) = s.last_frame {
            s.frames += 1;
            s.worst_frame = s.worst_frame.max(now.duration_since(last).as_nanos() as u64);
            s.runtime_worst_frame = s.runtime_worst_frame.max(runtime);
        }
        for m in s.mods.values_mut() {
            m.worst_frame = m.worst_frame.max(m.frame);
            m.frame = 0;
        }
        s.last_frame = Some(now);
    }
}

/// Start (from zero) or stop.
pub fn set(on: bool) {
    if on {
        reset();
    }
    ON.store(on, Ordering::Relaxed);
}

pub fn reset() {
    for c in &RUNTIME {
        c.reset();
    }
    RUNTIME_FRAME.store(0, Ordering::Relaxed);
    *STATE.lock().unwrap() = Some(State {
        started: Instant::now(),
        frames: 0,
        last_frame: None,
        worst_frame: 0,
        runtime_worst_frame: 0,
        mods: HashMap::new(),
    });
}

pub struct SiteReport {
    pub kind: &'static str,
    pub stat: Stat,
}

pub struct ModReport {
    pub id: String,
    pub ns: u64,
    pub calls: u64,
    pub worst_frame: u64,
    pub sites: Vec<SiteReport>,
}

pub struct Report {
    pub on: bool,
    pub seconds: f64,
    pub frames: u64,
    pub worst_frame: u64,
    pub runtime_worst_frame: u64,
    /// (name, calls, ns, max ns)
    pub runtime: Vec<(&'static str, u64, u64, u64)>,
    /// Most expensive first.
    pub mods: Vec<ModReport>,
}

pub fn report() -> Report {
    let guard = STATE.lock().unwrap();
    let runtime = RUNTIME
        .iter()
        .zip(RUNTIME_NAMES)
        .map(|(c, n)| (n, c.calls.load(Ordering::Relaxed), c.ns.load(Ordering::Relaxed), c.max.load(Ordering::Relaxed)))
        .collect();
    let Some(s) = guard.as_ref() else {
        return Report { on: enabled(), seconds: 0.0, frames: 0, worst_frame: 0, runtime_worst_frame: 0, runtime, mods: Vec::new() };
    };
    let mut mods: Vec<ModReport> = s
        .mods
        .iter()
        .map(|(id, m)| {
            let mut sites: Vec<SiteReport> = m.sites.iter().map(|(site, st)| SiteReport { kind: site.kind(), stat: st.clone() }).collect();
            sites.sort_by_key(|s| std::cmp::Reverse(s.stat.ns));
            ModReport {
                id: if id.is_empty() { "(runtime)".into() } else { id.clone() },
                ns: m.sites.values().map(|st| st.ns).sum(),
                calls: m.sites.values().map(|st| st.calls).sum(),
                worst_frame: m.worst_frame.max(m.frame),
                sites,
            }
        })
        .collect();
    mods.sort_by_key(|m| std::cmp::Reverse(m.ns));
    Report {
        on: enabled(),
        seconds: s.started.elapsed().as_secs_f64(),
        frames: s.frames,
        worst_frame: s.worst_frame,
        runtime_worst_frame: s.runtime_worst_frame,
        runtime,
        mods,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spin(us: u64) {
        let t = Instant::now();
        while t.elapsed().as_micros() < us as u128 {}
    }

    #[test]
    fn nested_entries_keep_their_own_time() {
        set(true);
        lua("outer", Site::Other, || "outer".into(), || {
            spin(2000);
            lua("inner", Site::Hook(7), || "Engine.HUD.PostRender".into(), || spin(3000));
            runtime(Runtime::EventHook, || spin(1000));
        });
        frame();
        let r = report();
        set(false);
        let get = |id: &str| r.mods.iter().find(|m| m.id == id).map(|m| m.ns).unwrap();
        let ms = |ns: u64| ns as f64 / 1e6;
        assert!((1.9..2.6).contains(&ms(get("outer"))), "outer {}", ms(get("outer")));
        assert!((2.9..3.6).contains(&ms(get("inner"))), "inner {}", ms(get("inner")));
        assert!((0.9..1.6).contains(&ms(r.runtime[0].2)), "runtime {}", ms(r.runtime[0].2));
        let inner = r.mods.iter().find(|m| m.id == "inner").unwrap();
        assert_eq!(inner.sites[0].stat.label, "Engine.HUD.PostRender");
        assert_eq!(inner.sites[0].kind, "hook");
        assert_eq!(inner.calls, 1);
    }
}
