//! What the runtime's hot entry points cost, in ns per call. Not a pass/fail test:
//! `cargo test --release --target x86_64-pc-windows-msvc --test hot_paths -- --ignored --nocapture`.
#![cfg(windows)]

use std::hint::black_box;
use std::time::Instant;

use tempest_scripting::{memory, unreal};

#[inline(never)]
extern "C" fn hook_target(a: usize, b: usize) -> usize {
    let mut x = a;
    for _ in 0..3 {
        x = black_box(x).wrapping_add(b);
    }
    x
}

fn time(what: &str, n: u64, mut f: impl FnMut()) {
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    println!("{what:<44} {:>9.1} ns", t.elapsed().as_nanos() as f64 / n as f64);
}

#[test]
#[ignore]
fn hot_paths() {
    let game = std::env::temp_dir().join(format!("tempest-scripting-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&game);
    std::fs::create_dir_all(game.join("ChaosGame/Config")).unwrap();
    std::fs::write(game.join("ChaosGame/Config/tempest.toml"), "[dev]\nhot_reload = false\n").unwrap();
    std::env::set_var("TEMPEST_GAME_DIR", &game);
    let _ = unreal::resolve();
    let lua = tempest_scripting::lua();
    assert!(lua.load("return tempest._ui_headless(1280, 720)").eval::<bool>().unwrap());

    let value = 1.5f32;
    time("memory::try_read (rust)", 200_000, || {
        black_box(memory::try_read::<f32>(black_box(&value as *const f32 as usize)));
    });
    time("memory::probe (rust)", 200_000, || {
        black_box(memory::probe(black_box(&value as *const f32 as usize), 4));
    });
    let block = vec![0usize; 256];
    let fake = block.as_ptr() as usize;
    time("unreal::find_property, cached (rust)", 200_000, || {
        black_box(unreal::find_property(black_box(fake), "m_fHealingTimer"));
    });

    let lua_loop = |what: &str, n: u64, setup: &str, body: &str| {
        let src = format!("{setup}\nlocal t = os.clock()\nfor i = 1, {n} do {body} end\nreturn (os.clock() - t) * 1e9 / {n}");
        let ns: f64 = lua.load(&src).eval().unwrap_or_else(|e| panic!("{what}: {e}"));
        println!("{what:<44} {ns:>9.1} ns");
    };
    let mem = "local a = memory.alloc(0x400) for o = 0, 0x3f8, 8 do memory.write_i64(a + o, a) end";
    lua_loop("memory.read_f32 (lua)", 500_000, mem, "local v = memory.read_f32(a + 16)");
    lua_loop("memory.read_ptr (lua)", 500_000, mem, "local v = memory.read_ptr(a + 16)");
    lua_loop("memory.write_f32 (lua)", 500_000, mem, "memory.write_f32(a + 16, 2.5)");
    lua_loop("memory.readable 0x1e0, our buffer (lua)", 200_000, mem, "local v = memory.readable(a, 0x1e0)");
    let big = vec![1u8; 512 << 20];
    let heap = format!("local a = {}", big.as_ptr() as usize + (256 << 20));
    lua_loop("memory.readable 0x1e0, large block (lua)", 5_000, &heap, "local v = memory.readable(a, 0x1e0)");
    lua_loop("memory.read_f32, large block (lua)", 500_000, &heap, "local v = memory.read_f32(a)");
    lua_loop("memory.readable, bad address (lua)", 20_000, "", "local v = memory.readable(0x10, 4)");
    lua_loop("memory.read_str 32 chars (lua)", 50_000, "local a = memory.alloc(64) memory.write_str(a, string.rep('x', 32))", "local v = memory.read_str(a)");
    lua_loop("tempest.object (lua)", 200_000, mem, "local v = tempest.object(a)");
    lua_loop("tempest._with_mod (lua)", 500_000, "local f = function() end", "tempest._with_mod('some-mod', f)");
    lua_loop("UI frame, nothing to draw (lua)", 20_000, "", "tempest._ui_build_frame()");
    lua_loop("UI frame, 100 empty handlers (lua)", 5_000, "tempest._with_mod('some-mod', function() for i = 1, 100 do ui.on_draw(function() end) end end)", "tempest._ui_build_frame()");

    let target = hook_target as extern "C" fn(usize, usize) -> usize as usize;
    let setup = format!("local target = {target}");
    lua_loop("memory.call, not hooked (lua)", 200_000, &setup, "memory.call(target, 1, 2)");
    let hooked = format!("{setup}\nlocal id = memory.hook(target, function(orig, a, b) end, {{ args = 2 }})\n_G.bench_id = id");
    lua_loop("memory.call, hook passes through (lua)", 200_000, &hooked, "memory.call(target, 1, 2)");
    lua.load("memory.unhook(_G.bench_id)").exec().unwrap();
    let hooked = format!("{setup}\n_G.bench_id = memory.hook(target, function(orig, a, b) return orig(a, b) end, {{ args = 2 }})");
    lua_loop("memory.call, hook calls original (lua)", 200_000, &hooked, "memory.call(target, 1, 2)");
    lua.load("memory.unhook(_G.bench_id)").exec().unwrap();
}
