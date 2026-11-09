//! Timing of the runtime's own Lua paths on the host (`cargo test --release
//! --test perf_bench -- --ignored --nocapture`): the performance window with
//! a dozen mods in it, and a draw handler of plain text widgets.
use std::time::Instant;

#[test]
#[ignore]
fn perf_window_and_text_widgets() {
    let game = std::env::temp_dir().join(format!("tempest-scripting-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&game);
    std::fs::create_dir_all(game.join("ChaosGame/Config")).unwrap();
    std::fs::write(game.join("ChaosGame/Config/tempest.toml"), "[dev]\nhot_reload = false\n").unwrap();
    std::env::set_var("TEMPEST_GAME_DIR", &game);
    let _ = tempest_scripting::unreal::resolve();
    let lua = tempest_scripting::lua();
    let eval = |src: &str| lua.load(src).eval::<mlua::Value>().expect(src);
    assert!(eval("return tempest._ui_headless(1920, 1080)").as_boolean().unwrap());
    std::env::set_var("TEMPEST_SCRIPT_DIR", concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/scripting/scripts"));
    tempest_scripting::loader::run_all(lua);
    std::env::remove_var("TEMPEST_SCRIPT_DIR");

    let frames = |n: usize| {
        let t = Instant::now();
        for _ in 0..n {
            eval("return tempest._ui_build_frame()");
        }
        t.elapsed().as_secs_f64() * 1000.0 / n as f64
    };
    // A dozen mods with a few entries each, so the window has ~40 rows.
    eval("for i = 1, 12 do tempest._with_mod('mod-' .. i, function() for j = 1, 3 do ui.on_draw(function() end) end end) end; return true");
    frames(200);
    let base = frames(2000);
    // Open the window (Ctrl+Shift+P) and expand every tree node.
    eval("tempest._key(0x50, true, true, false, true, false); return true");
    eval("ui.on_draw(function() end); return true");
    frames(50);
    // Every mod expanded, as in a real look at the table.
    eval("local tn = ui.tree_node; ui.tree_node = function(...) ui.set_next_item_open(true) return tn(...) end; return true");
    frames(50);
    let window = frames(2000);
    eval("tempest._key(0x50, true, true, false, true, false); return true");
    println!("frame with 36 empty handlers: {base:.4} ms; with the perf window open: {window:.4} ms (window: {:.4} ms)", window - base);

    eval("shared.pts = require('ffi').new('ImVec2[4]'); return true");
    // A handler drawing 100 lines of text, plain / coloured / disabled.
    for (name, call) in [("ui.text", "ui.text('value %d', i)"), ("ui.text_colored", "ui.text_colored(1, 0.5, 0.5, 1, 'value %d', i)"), ("ui.text_disabled", "ui.text_disabled('value %d', i)"), ("ui.color", "local c = ui.color(1, 0.5, i / 100, 1)"), ("ui.fg.line", "ui.fg.line(i, 0, i, 100, 0xff00ff00, 2)"), ("ui.fg.rect_filled + ui.color", "ui.fg.rect_filled(i, 0, i + 5, 5, ui.color(1, 0, 0, 0.5))"), ("ui.fg.text", "ui.fg.text(i, 10, 0xffffffff, 'hp %d', i)"), ("polyline via pointer", "local p = shared.pts; p[0].x, p[0].y, p[1].x, p[1].y = i, 0, i, 100; ui.C.ImDrawList_AddPolyline(ui.C.igGetForegroundDrawList_Nil(), p, 2, 0xff00ff00, 0, 2)")] {
        eval(&format!("shared.bench = ui.on_draw(function() ui.window('Bench', function() for i = 1, 100 do {call} end end) end); return true"));
        frames(200);
        let t = frames(2000);
        let verts = eval("return ui.stats().vertices");
        eval("ui.off_draw(shared.bench); return true");
        println!("  vertices {verts:?}");
        println!("100 x {name}: {:.4} ms a frame ({:.2} us each)", t - base, (t - base) * 10.0);
    }
}

/// Garbage from handlers (userdata like the Object proxies every field read
/// returns, plus strings): how long the worst frame takes.
#[test]
#[ignore]
fn gc_spikes_under_userdata_churn() {
    let lua = tempest_scripting::lua();
    let eval = |src: &str| lua.load(src).eval::<mlua::Value>().expect(src);
    // 12 "mods", each making 300 userdata and some strings a frame.
    eval("_G.bench_mods = {} for m = 1, 12 do _G.bench_mods[m] = function() local t = {} for i = 1, 300 do t[#t + 1] = memory.f32(i) end local s = string.format('hp %d', m) return t, s end end return true");
    let frame = || {
        let t = std::time::Instant::now();
        eval("for _, f in ipairs(_G.bench_mods) do f() end return true");
        t.elapsed().as_secs_f64() * 1000.0
    };
    for _ in 0..500 {
        frame();
    }
    let mut times: Vec<f64> = (0..5000).map(|_| frame()).collect();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg = times.iter().sum::<f64>() / times.len() as f64;
    println!("avg {avg:.3} ms, p99 {:.3} ms, worst {:.3} ms", times[times.len() * 99 / 100], times[times.len() - 1]);
}

/// What timing one hooked call costs the profiler (it times ~50 000 a second
/// in a match), against the call untimed.
#[test]
#[ignore]
fn profiler_overhead_per_call() {
    use tempest_scripting::profile::{runtime, set, Runtime};
    let n = 2_000_000u32;
    let run = || {
        let t = std::time::Instant::now();
        for i in 0..n {
            std::hint::black_box(runtime(Runtime::ScriptHook, || std::hint::black_box(i)));
        }
        t.elapsed().as_nanos() as f64 / n as f64
    };
    set(false);
    let off = run();
    set(true);
    let on = run();
    set(false);
    println!("per call: {off:.1} ns untimed, {on:.1} ns timed");
}

/// One event delivered to a Lua handler that does nothing: the runtime's
/// share of every PostRender a mod subscribes to.
#[test]
#[ignore]
fn delivery_overhead() {
    let lua = tempest_scripting::lua();
    let id: u64 = lua.load("return tempest.on('PostRender', function(this, params) end)").eval().unwrap();
    let run = |n: u32| {
        let t = std::time::Instant::now();
        for i in 0..n {
            tempest_scripting::lua_api::bench_deliver(id, "some-mod", 0x1000 + (i as usize % 4) * 0x40);
        }
        t.elapsed().as_nanos() as f64 / n as f64
    };
    run(20_000);
    let off = run(200_000);
    tempest_scripting::profile::set(true);
    let on = run(200_000);
    tempest_scripting::profile::set(false);
    println!("delivery: {off:.0} ns, {on:.0} ns while measuring");
}

/// The pieces of a delivery: a Rust -> Lua call, creating a userdata.
#[test]
#[ignore]
fn delivery_pieces() {
    let lua = tempest_scripting::lua();
    let f: mlua::Function = lua.load("return function(a, b) end").eval().unwrap();
    let n = 200_000u32;
    let time = |name: &str, g: &dyn Fn(u32)| {
        for i in 0..20_000 {
            g(i);
        }
        let t = std::time::Instant::now();
        for i in 0..n {
            g(i);
        }
        println!("{name}: {:.0} ns", t.elapsed().as_nanos() as f64 / n as f64);
    };
    time("call (nil, nil)", &|_| f.call::<()>((mlua::Value::Nil, mlua::Value::Nil)).unwrap());
    time("create_userdata(Object)", &|i| {
        std::hint::black_box(lua.create_userdata(tempest_scripting::lua_api::Object(i as usize)).unwrap());
    });
    time("call with 2 userdata", &|i| {
        let a = lua.create_userdata(tempest_scripting::lua_api::Object(i as usize)).unwrap();
        let b = lua.create_userdata(tempest_scripting::lua_api::Object(i as usize)).unwrap();
        f.call::<()>((a, b)).unwrap()
    });
    let ud = lua.create_userdata(tempest_scripting::lua_api::Object(1)).unwrap();
    time("ud.borrow::<Object>()", &|_| {
        std::hint::black_box(ud.borrow::<tempest_scripting::lua_api::Object>().unwrap().0);
    });
    let tbl = lua.create_table().unwrap();
    tbl.raw_set(1, &ud).unwrap();
    time("table.raw_get", &|_| {
        std::hint::black_box(tbl.raw_get::<mlua::Value>(1).unwrap());
    });
    time("ud.clone()", &|_| {
        std::hint::black_box(ud.clone());
    });
    time("handler lookup (registry_value)", &|_| {
        let key = lua.create_registry_value(f.clone()).unwrap();
        std::hint::black_box(lua.registry_value::<mlua::Function>(&key).unwrap());
    });
    time("with_mod_at", &|_| tempest_scripting::loader::with_mod_at("some-mod", tempest_scripting::profile::Site::Other, String::new, || ()));
}

/// An empty UI frame, and one per empty draw handler on top.
#[test]
#[ignore]
fn ui_frame_parts() {
    let lua = tempest_scripting::lua();
    let eval = |src: &str| lua.load(src).eval::<mlua::Value>().expect(src);
    assert!(eval("return tempest._ui_headless(1920, 1080)").as_boolean().unwrap());
    let frames = |n: usize| {
        let t = std::time::Instant::now();
        for _ in 0..n {
            tempest_scripting::overlay::build_frame();
        }
        t.elapsed().as_secs_f64() * 1e6 / n as f64
    };
    frames(500);
    let empty = frames(5000);
    eval("for i = 1, 7 do tempest._with_mod('m' .. i, function() ui.on_draw(function() end) end) end return true");
    frames(500);
    let seven = frames(5000);
    println!("UI frame: {empty:.1} us empty, {seven:.1} us with 7 empty handlers ({:.2} us each)", (seven - empty) / 7.0);
}
