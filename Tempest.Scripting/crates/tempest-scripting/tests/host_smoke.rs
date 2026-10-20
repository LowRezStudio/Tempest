//! The whole Lua surface must load and behave on a host with no game image:
//! every native lookup reports "unresolved" instead of panicking.

use std::collections::HashMap;

use tempest_scripting::unreal;

#[test]
fn layouts_match_the_sdk() {
    use unreal::layout::*;
    assert_eq!((CPF_PARM, CPF_OUT_PARM, CPF_RETURN_PARM), (0x80, 0x100, 0x400));
    if PTR == 8 {
        // Win64 client (8.1 SDK layouts).
        assert_eq!((UOBJECT_OUTER, UOBJECT_NAME, UOBJECT_CLASS), (0x40, 0x48, 0x50));
        assert_eq!((UFIELD_NEXT, USTRUCT_SUPER, USTRUCT_CHILDREN), (0x60, 0x78, 0x80));
        assert_eq!((UPROP_FLAGS, UPROP_OFFSET, UPROP_EXT), (0x70, 0x7C, 0x98));
        assert_eq!((CHEAT_ARRAY_COUNT, TARRAY_COUNT, FNAME_ENTRY_NAME), (0x0C, 0x08, 0x14));
        assert_eq!((UFUNCTION_FUNC, UFUNCTION_SCRIPT_DATA, UFUNCTION_PARMS_SIZE), (0xF0, 0x8C, 0xE2));
        assert_eq!((FRAME_NODE, FRAME_OBJECT, FRAME_CODE, FRAME_LOCALS), (0x14, 0x1C, 0x24, 0x2C));
    } else {
        // Win32 client (Paladins-32.exe in Binary Ninja).
        assert_eq!((UOBJECT_OUTER, UOBJECT_NAME, UOBJECT_CLASS), (0x28, 0x2C, 0x34));
        assert_eq!((UFIELD_NEXT, USTRUCT_SUPER, USTRUCT_CHILDREN), (0x3C, 0x48, 0x4C));
        assert_eq!((UPROP_FLAGS, UPROP_OFFSET, UPROP_EXT), (0x48, 0x54, 0x64));
        assert_eq!((CHEAT_ARRAY_COUNT, TARRAY_COUNT, FNAME_ENTRY_NAME), (0x08, 0x04, 0x10));
        assert_eq!((UFUNCTION_FUNC, UFUNCTION_SCRIPT_DATA, UFUNCTION_PARMS_SIZE), (0xA0, 0x54, 0x96));
        assert_eq!((FRAME_NODE, FRAME_OBJECT, FRAME_CODE, FRAME_LOCALS), (0x10, 0x14, 0x18, 0x1C));
    }
    // Every pointer-bearing object offset is pointer aligned (FFrame is 4-byte packed).
    for off in [UOBJECT_OUTER, UOBJECT_CLASS, UFIELD_NEXT, USTRUCT_SUPER, USTRUCT_CHILDREN, UPROP_EXT, UFUNCTION_FUNC, OUTPARM_ADDR, OUTPARM_NEXT] {
        assert_eq!(off % PTR, 0, "offset 0x{off:x}");
    }
    for sig in unreal::signatures() {
        tempest_scripting::memory::parse_pattern(sig).unwrap();
    }
}

/// A fake game folder: a dev mod in mods/, settings and mod configs under ChaosGame/Config.
fn fake_game_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tempest-scripting-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("mods/devmod/scripts")).unwrap();
    std::fs::create_dir_all(dir.join("ChaosGame/Config")).unwrap();
    std::fs::write(
        dir.join("mods/devmod/scripts/main.lua"),
        "local cfg = tempest.config({ speed = 2, greeting = 'hi', nested = { on = true }, list = { 1, 2, 3 } })\n\
         shared.devmod = { speed = cfg.speed, greeting = cfg.greeting, on = cfg.nested.on, n = #cfg.list, path = tempest.config_path() }\n",
    )
    .unwrap();
    std::fs::write(dir.join("ChaosGame/Config/tempest.toml"), "[dev]\nhot_reload = false\n[ui]\nscale = 1.5\n").unwrap();
    // The launcher's registry: a Lua V2 mod, a disabled Lua V2 mod, a DLL+content V2 mod, a V1 asset mod.
    std::fs::create_dir_all(dir.join(".tempest/mods")).unwrap();
    let mut manifest_hashes: HashMap<&str, String> = HashMap::new();
    for (id, scripts, dll, files) in [("lua-mod", true, false, false), ("off-mod", true, false, false), ("skinpack", false, true, true)] {
        let m = dir.join(".tempest/v2/mods").join(id);
        std::fs::create_dir_all(&m).unwrap();
        if scripts {
            let body = format!("shared.ran_{} = true\n", id.replace('-', "_"));
            std::fs::create_dir_all(m.join("scripts")).unwrap();
            std::fs::write(m.join("scripts/main.lua"), &body).unwrap();
            // Signed installs measured this file into CHECKSUMS at install time
            // (the signature proves CHECKSUMS, the CLI re-checks files at launch).
            let checksums = format!("{} scripts/main.lua\n", sha256_hex(body.as_bytes()));
            std::fs::write(m.join("CHECKSUMS"), &checksums).unwrap();
            manifest_hashes.insert(id, sha256_hex(checksums.as_bytes()));
        }
        if dll {
            std::fs::create_dir_all(m.join("dlls")).unwrap();
            std::fs::write(m.join("dlls/Thing.dll"), "MZ").unwrap();
        }
        if files {
            std::fs::create_dir_all(m.join("files/ChaosGame/CookedPCConsole")).unwrap();
            std::fs::write(m.join("files/ChaosGame/CookedPCConsole/Skin.upk"), "x").unwrap();
        }
    }
    std::fs::write(dir.join(".tempest/mods/mods.json"), format!(
        r#"[
      {{"Id":"lua-mod","Name":"Lua Mod","Version":"1.2.0","Authors":[{{"Name":"Tom"}}],"Enabled":true,"Kind":"V2","InstalledFiles":[],"Signer":"7dbfdf2626dd1c26c0d7fda307e4d5f76f4b5b8db9dea9683a6f5f4d3b3fbe4f","Verified":true,"ManifestHash":"{}"}},
      {{"Id":"off-mod","Name":"Off","Version":"0.1","Enabled":false,"Kind":"V2","InstalledFiles":[]}},
      {{"Id":"skinpack","Name":"Skins","Version":"2.0","Author":"Someone","Enabled":true,"Kind":"V2","InstalledFiles":["ChaosGame/CookedPCConsole/Skin.upk"]}},
      {{"Id":"oldvoice","Name":"Old voice pack","Version":"1.0","Enabled":true,"Kind":"Voice","InstalledFiles":["a.upk","b.upk"]}}
    ]"#,
        manifest_hashes["lua-mod"],
    )).unwrap();
    dir
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(data)
        .iter()
        .flat_map(|b| [(HEX[(b >> 4) as usize]) as char, (HEX[(b & 0xf) as usize]) as char])
        .collect()
}

#[test]
fn host_smoke() {
    let game = fake_game_dir();
    std::env::set_var("TEMPEST_GAME_DIR", &game);
    let g = unreal::resolve();
    assert!(!g.gnames_ok && g.process_event == 0 && !g.notes.is_empty());

    let lua = tempest_scripting::lua();
    let eval = |src: &str| lua.load(src).eval::<mlua::Value>().expect(src);

    // The VM is LuaJIT with the 5.2 compatibility surface; ffi/bit/jit load
    // through `require` (also from inside a mod sandbox, see below).
    assert_eq!(eval("return _VERSION"), mlua::Value::String(lua.create_string("Lua 5.1").unwrap()));
    assert!(eval("return jit.version:match('^LuaJIT') ~= nil").as_boolean().unwrap());
    assert!(eval("return type(require('ffi').cast) == 'function' and type(require('bit').band) == 'function'").as_boolean().unwrap());
    assert!(eval("return type(table.pack) == 'function' and type(table.unpack) == 'function'").as_boolean().unwrap());
    // Integral doubles arrive as integers (0x7fff_ffff_ffff is a plausible pointer).
    assert_eq!(eval("return 0x7fffffffffff"), mlua::Value::Integer(0x7fff_ffff_ffff));
    assert_eq!(eval("return string.format('0x%x', 0x7fffffffffff)"), mlua::Value::String(lua.create_string("0x7fffffffffff").unwrap()));

    assert!(eval("return tempest.status().gobjects").is_nil());
    assert_eq!(eval("return tempest.status().gnames_ok"), mlua::Value::Boolean(false));
    assert!(eval("return #tempest.status().notes > 0").as_boolean().unwrap());
    assert!(eval("return tempest.find_class('TgGame.TgGame')").is_nil());
    assert!(eval("return tempest.local_player()").is_nil());
    assert_eq!(eval("return #tempest.objects(8)"), mlua::Value::Integer(0));
    assert_eq!(eval("return #memory.scan('48 8B 05 ? ? ? ?')"), mlua::Value::Integer(0));
    assert!(eval("return memory.read_u32(0)").is_nil());

    // Subscriptions register without a game and clear on reload. (The
    // prelude already holds one: the ConsoleCommand hook behind tempest.command.)
    eval("local id = tempest.on('TgGame.TgGame.PostLogin', function() end); \
          assert(tempest.status().subscriptions[id] == 'TgGame.TgGame.PostLogin'); tempest.off(id); return true");
    eval("local id = tempest.before('TakeDamage', function() return false end); \
          assert(tempest.status().subscriptions[id] == 'before TakeDamage'); tempest.off(id); return true");
    assert_eq!(unreal::subscriptions().len(), 1);
    eval("tempest.command('probe', function() end, 'help'); \
          local found = false; for _, c in ipairs(tempest.commands()) do found = found or c.name == 'probe' end; \
          assert(found); return true");

    // Queued jobs stay queued (no game thread ever fires on the host).
    eval("tempest.queue(function() end); return true");
    assert!(eval("return tempest.on_game_thread()") == mlua::Value::Boolean(false));
    assert!(eval("return tempest.status().game_thread").is_nil());
    assert!(eval("return tempest.status().process_internal").is_nil());
    assert!(eval("return tempest.new('Engine.Console', nil)").is_nil());
    // Key specs parse; the hook itself cannot install on the host.
    eval("local id = tempest.on_key('Ctrl+Shift+F5', function() return true end); tempest.off_key(id); return true");
    assert!(eval("return pcall(tempest.on_key, 'Ctrl+Bogus', function() end)") == mlua::Value::Boolean(false));
    assert_eq!(eval("return tempest.status().key_hook_installed"), mlua::Value::Boolean(false));
    // Sandbox + shared: run the lib mod from its folder and import from it.
    std::env::set_var("TEMPEST_SCRIPT_DIR", concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/lib/scripts"));
    tempest_scripting::loader::run_all(lua);
    std::env::remove_var("TEMPEST_SCRIPT_DIR");
    assert!(eval("return type(tempest.import('greet')) == 'function'").as_boolean().unwrap());
    assert!(eval("return LIB_PRIVATE == nil").as_boolean().unwrap());
    assert!(eval("return shared.lib_ffi_ok == true").as_boolean().unwrap());
    assert_eq!(eval("return shared.__providers.greet"), mlua::Value::String(lua.create_string("local").unwrap()));
    assert_eq!(eval("return #tempest.status().native_hooks"), mlua::Value::Integer(0));

    // Address hooks: refused on the host (no code to detour), bookkeeping intact.
    assert_eq!(eval("return pcall(memory.hook, 0x1000, function() end)"), mlua::Value::Boolean(false));
    assert_eq!(eval("return pcall(memory.hook, 0x1000, function() end, { args = 9 })"), mlua::Value::Boolean(false));
    assert_eq!(eval("return #memory.hooks() + #tempest.status().address_hooks"), mlua::Value::Integer(0));
    assert_eq!(eval("return memory.unhook(42)"), mlua::Value::Boolean(false));
    assert!(eval("return memory.call(0, 1, 2)").is_nil());
    assert!(eval("return memory.call(0, 1, memory.f32(1.5), { ret = 'f64' })").is_nil());
    assert_eq!(eval("return tostring(memory.f32(1.5))"), mlua::Value::String(lua.create_string("f32(1.5)").unwrap()));
    assert_eq!(eval("return pcall(memory.hook, 0x1000, function() end, { args = { 'int', 'bogus' } })"), mlua::Value::Boolean(false));
    assert_eq!(eval("return pcall(memory.call, 0, 1, { ret = 'nope' })"), mlua::Value::Boolean(false));
    // Typed memory access: readable on our own buffer, nil elsewhere.
    assert!(eval("local p = memory.alloc(16); memory.write_c(p, 'uint64_t', 0x123456789ABCDEFULL); local v = memory.read_c(p, 'uint64_t'); memory.free(p); return v == 0x123456789ABCDEFULL").as_boolean().unwrap());
    assert!(eval("return memory.read_c(0, 'int') == nil and memory.write_c(0, 'int', 1) == false").as_boolean().unwrap());
    assert!(eval("return tempest.default_object('Engine.Actor')").is_nil());
    assert_eq!(eval("return pcall(tempest.spawn, 'Engine.PointLight', nil, nil, nil, nil, { force = true })"), mlua::Value::Boolean(false));
    assert!(eval("return tempest.object(0)").is_nil());
    // Scratch buffers work everywhere (plain heap memory); memory.read_* stays
    // stubbed on the host, so the round trip is checked from Rust.
    let p = eval("local p = memory.alloc(64); assert(p > 0); \
          assert(memory.write_wstr(p, 'héllo')); assert(not memory.write_wstr(p + 60, 'too long')); \
          return p").as_integer().unwrap() as usize;
    let units: Vec<u16> = (0..5).map(|i| unsafe { std::ptr::read((p + i * 2) as *const u16) }).collect();
    assert_eq!(String::from_utf16_lossy(&units), "héllo");
    assert_eq!(unsafe { std::ptr::read((p + 10) as *const u16) }, 0);
    eval(&format!("assert(memory.write_str({p}, 'abc')); assert(memory.free({p})); assert(not memory.free({p})); return true"));
    assert_eq!(eval("return pcall(memory.alloc, 0)"), mlua::Value::Boolean(false));
    // Lua natives: a class that is not loaded registers lazily; a bare name is an error.
    assert_eq!(eval("return pcall(tempest.native, 'NoClassHere', function() end)"), mlua::Value::Boolean(false));
    eval("local id = tempest.native('Mod.Class.Func', function(this, p) return 1 end); \
          assert(tempest.status().subscriptions[id] == 'before Mod.Class.Func'); tempest.off(id); return true");
    assert_eq!(eval("return #tempest.status().lua_natives"), mlua::Value::Integer(0));
    assert_eq!(eval("return memory.write_str(0, 'x')"), mlua::Value::Boolean(false));

    // UI: a backend-less ImGui context; the Lua layer builds real frames.
    assert_eq!(eval("return ui.backend()"), mlua::Value::String(lua.create_string("").unwrap()));
    assert_eq!(eval("return ui.WindowFlags.NoTitleBar + ui.Cond.Once"), mlua::Value::Integer(1 + 2));
    assert!(eval("return require('ffi').string(ui.C.igGetVersion()) == '1.91.9'").as_boolean().unwrap());
    assert_eq!(eval("return pcall(ui.text, 'outside a frame')"), mlua::Value::Boolean(false));
    assert!(eval("return tempest._ui_headless(1280, 720) and ui.ready()").as_boolean().unwrap());
    eval("ui.on_draw(function() \
            ui.window('Test', function() \
                ui.text('hello %d', 42) \
                local v, c = ui.slider_float('s', 0.5, 0, 1); assert(v == 0.5 and c == false) \
                local s, ch = ui.input_text('t', 'abc'); assert(s == 'abc' and ch == false) \
                local i = ui.combo('c', 2, { 'a', 'b', 'c' }); assert(i == 2) \
                local col = ui.color_edit('col', { 1, 0.5, 0, 1 }); assert(#col == 4 and col[2] > 0.49 and col[2] < 0.51) \
                ui.table('tbl', 2, function() ui.table_next_row(); ui.table_next_column(); ui.text('a'); ui.table_next_column(); ui.text('b') end) \
                ui.fg.line(0, 0, 10, 10, ui.rgb(0xff0000)) \
                ui.dl.rect_filled(0, 0, 5, 5, ui.color(0, 1, 0, 0.5)) \
            end) \
          end); return true");
    assert!(eval("return tempest._ui_build_frame()").as_boolean().unwrap());
    assert!(eval("local s = ui.stats(); return s.built == 1 and s.vertices > 0 and s.lists > 0").as_boolean().unwrap());
    // Failing handlers are recovered (window stack rebalanced) and the frame still completes.
    eval("ui.on_draw(function() ui.window('Broken', function() ui.text('x'); error('boom') end) end); \
          ui.on_draw(function() error('bad handler') end); return true");
    assert!(eval("return tempest._ui_build_frame() and tempest._ui_build_frame()").as_boolean().unwrap());
    assert!(eval("return ui.stats().built == 3").as_boolean().unwrap());
    // Fonts: the built-in font at another size marks the atlas dirty; the headless
    // frame builder rebuilds it and the font can be pushed.
    assert!(eval("local f = ui.add_font(nil, 20); shared.font = f; return f ~= nil and tempest._ui_fonts_dirty()").as_boolean().unwrap());
    eval("ui.on_draw(function() ui.window('Fonts', function() ui.with_font(shared.font, function() ui.text('big') end) end) end); return true");
    assert!(eval("return tempest._ui_build_frame() and not tempest._ui_fonts_dirty()").as_boolean().unwrap());
    assert!(eval("return ui.add_font('/nonexistent/font.ttf', 16) == nil").as_boolean().unwrap());
    // Images: raw RGBA pixels become a texture once "uploaded"; ui.image draws it.
    eval("local px = string.rep('\\255\\0\\0\\255', 4); shared.img = ui.image_from_rgba(2, 2, px); \
          ui.on_draw(function() ui.window('Img', function() ui.image(shared.img, 50, 50); ui.fg.image(shared.img, 0, 0, 10, 10) end) end); return true");
    assert!(eval("return shared.img ~= nil and shared.img.width == 2 and not shared.img:ready()").as_boolean().unwrap());
    assert!(eval("return tempest._ui_build_frame()").as_boolean().unwrap());
    assert_eq!(eval("return tempest._ui_headless_upload()"), mlua::Value::Integer(1));
    assert!(eval("return shared.img:ready() and tempest._ui_build_frame()").as_boolean().unwrap());
    eval("shared.img:free(); return true");
    assert!(eval("return not shared.img:ready() and ui.load_image('/nonexistent.png') == nil").as_boolean().unwrap());

    // Reload re-runs ui.lua on the cached FFI bindings; frames keep building.
    assert!(eval("tempest.reload(); return ui.C ~= nil and tempest._ui_build_frame()").as_boolean().unwrap());
    assert_eq!(eval("return #tempest.status().subscriptions > 0 and ui.capturing()"), mlua::Value::Boolean(false));

    // Runtime settings and dev mods: the fake game folder's tempest.toml was read,
    // the dev mod in mods/ loads with its config defaults written to Config/mods.
    assert!(eval("local s = tempest.settings(); return s.hot_reload == false and s.ui_scale == 1.5 and s.dev_mods ~= nil").as_boolean().unwrap());
    assert_eq!(eval("return ui.scale()"), mlua::Value::Number(1.5));
    eval("tempest.reload(); return true");
    assert!(eval("return shared.devmod ~= nil and shared.devmod.speed == 2 and shared.devmod.greeting == 'hi' and shared.devmod.on == true and shared.devmod.n == 3").as_boolean().unwrap());
    // A dev mod's editable config is its own config.toml (installed mods use Config/mods/<id>.toml).
    let cfg = game.join("mods/devmod/config.toml");
    let written = std::fs::read_to_string(&cfg).expect("mod config written");
    assert!(written.contains("speed = 2") && written.contains("[nested]"), "{written}");
    assert!(eval("for _, id in ipairs(tempest.mods()) do if id == 'devmod' then return true end end return false").as_boolean().unwrap());
    // Installed mods from the registry: Lua ones run (enabled only), others are listed with their kind.
    assert!(eval("return shared.ran_lua_mod == true and shared.ran_off_mod == nil").as_boolean().unwrap());
    assert!(eval("local by = {}; for _, m in ipairs(tempest.installed()) do by[m.id] = m end; \
                  return by['lua-mod'].lua and by['lua-mod'].loaded and by['lua-mod'].authors[1] == 'Tom' and by['lua-mod'].version == '1.2.0' \
                     and by['off-mod'].lua and not by['off-mod'].enabled and not by['off-mod'].loaded \
                     and by['skinpack'].dll and not by['skinpack'].lua and by['skinpack'].files == 1 and by['skinpack'].authors[1] == 'Someone' \
                     and by['oldvoice'].kind == 'Voice' and by['oldvoice'].files == 2 and not by['oldvoice'].lua \
                     and by['devmod'].dev and by['devmod'].kind == 'dev' and by['devmod'].loaded").as_boolean().unwrap());
    // Modifying an installed mod's script after install: the CLI refuses it at the next
    // launch (marked unverified in the registry), and the runtime then does the same.
    let registry = game.join(".tempest/mods/mods.json");
    let json = std::fs::read_to_string(&registry).unwrap().replace("\"Verified\":true", "\"Verified\":false");
    std::fs::write(&registry, json).unwrap();
    std::fs::write(game.join(".tempest/v2/mods/lua-mod/scripts/main.lua"), "shared.ran_lua_mod = false\n").unwrap();
    eval("tempest.reload(); return true");
    // The reload's `shared` starts empty and the tampered script never runs.
    assert!(eval("local by = {}; for _, m in ipairs(tempest.installed()) do by[m.id] = m end return not by['lua-mod'].loaded").as_boolean().unwrap());
    assert_eq!(eval("return shared.ran_lua_mod"), mlua::Value::Nil);
    // The user edits the file; the mod sees the new value on reload (what hot reload triggers).
    std::fs::write(&cfg, "speed = 5\ngreeting = \"yo\"\n").unwrap();
    eval("tempest.reload('devmod'); return true");
    assert!(eval("return shared.devmod.speed == 5 and shared.devmod.greeting == 'yo' and shared.devmod.on == true").as_boolean().unwrap());
    // save_config round-trips a table from Lua.
    eval("tempest.reload('devmod'); return true");
    assert!(eval("return shared.devmod.path:find('config.toml', 1, true) ~= nil").as_boolean().unwrap());
    // The shipped hot-reload example runs as a dev mod too.
    let hot = game.join("mods/hot-reload");
    std::fs::create_dir_all(hot.join("scripts")).unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/hot-reload/scripts/main.lua"), hot.join("scripts/main.lua")).unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/hot-reload/config.toml"), hot.join("config.toml")).unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/hot-reload/manifest.toml"), hot.join("manifest.toml")).unwrap();
    eval("tempest.reload('hot-reload'); tempest.reload('hot-reload'); return true");
    assert_eq!(eval("return shared.hot_runs"), mlua::Value::Integer(2));
    assert_eq!(std::fs::read_to_string(hot.join("config.toml")).unwrap().lines().filter(|l| l.contains("label")).count(), 1);
    let hot_cfg = game.join("ChaosGame/Config/mods/hot-reload.toml");
    assert!(!hot_cfg.exists(), "dev mods get no Config/mods file");

    // Key bindings and commands run as the mod that registered them (tempest.config works inside).
    std::fs::write(hot.join("scripts/ctx.lua"), "tempest.on_key('F11', function() shared.key_mod = tempest.mod(); shared.key_cfg = tempest.config({ a = 1 }).a end) \
        tempest.command('ctxtest', function() shared.cmd_mod = tempest.mod() end) \
        tempest.queue(function() shared.queue_mod = tempest.mod() end)\n").unwrap();
    eval("tempest.reload('hot-reload'); tempest._key(0x7A, false, false, false, true, false); return true");
    assert!(eval("return shared.key_mod == 'hot-reload' and shared.key_cfg == 1").as_boolean().unwrap());
    std::fs::remove_file(hot.join("scripts/ctx.lua")).unwrap();

    // The fps-limit example loads as a dev mod (no engine on the host: nothing applied, no error).
    let fps = game.join("mods/fps-limit");
    std::fs::create_dir_all(fps.join("scripts")).unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/fps-limit/scripts/main.lua"), fps.join("scripts/main.lua")).unwrap();
    eval("tempest.reload('fps-limit'); return true");
    assert!(fps.join("config.toml").is_file());
    assert!(eval("for _, c in ipairs(tempest.commands()) do if c.name == 'fps' then return true end end return false").as_boolean().unwrap());

    // The watcher: a saved script or config file reloads its mod after the quiet period;
    // files outside a mod (types/) are ignored.
    let mut w = tempest_scripting::watch::Watcher::new(Some(game.join("mods")), Some(game.join("ChaosGame/Config/mods")), true, std::time::Duration::from_millis(50));
    assert!(w.tracked() >= 4);
    assert!(w.poll().is_empty());
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(hot.join("scripts/main.lua"), "shared.hot_runs = (shared.hot_runs or 0) + 1\nshared.hot_edited = true\n").unwrap();
    std::fs::create_dir_all(game.join("mods/types")).unwrap();
    std::fs::write(game.join("mods/types/x.lua"), "-- not a mod").unwrap();
    assert!(w.poll().is_empty(), "a change starts the quiet period");
    std::thread::sleep(std::time::Duration::from_millis(80));
    let ready = w.poll();
    assert_eq!(ready, vec!["hot-reload".to_string()]);
    tempest_scripting::watch::reload(ready);
    assert!(eval("return shared.hot_runs == 4 and shared.hot_edited == true").as_boolean().unwrap());
    // Editing the dev mod's config.toml reloads it and the new value is what the script sees.
    std::fs::write(hot.join("scripts/main.lua"), "local cfg = tempest.config({ label = 'default' })\nshared.hot_label = cfg.label\n").unwrap();
    std::fs::write(hot.join("config.toml"), "label = \"changed\"\n").unwrap();
    assert!(w.poll().is_empty());
    std::thread::sleep(std::time::Duration::from_millis(80));
    let ready = w.poll();
    assert_eq!(ready, vec!["hot-reload".to_string()]);
    tempest_scripting::watch::reload(ready);
    assert_eq!(eval("return shared.hot_label"), mlua::Value::String(lua.create_string("changed").unwrap()));
    assert!(w.poll().is_empty(), "nothing pending afterwards");
    // A config the mod writes itself (tempest.save_config) is not an edit.
    std::fs::write(hot.join("scripts/main.lua"), "local cfg = tempest.config({ label = 'default' })\nshared.hot_label = cfg.label\nshared.hot_save = function(v) cfg.label = v; tempest.save_config(cfg) end\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(80));
    while !w.poll().is_empty() {}
    std::thread::sleep(std::time::Duration::from_millis(80));
    let ready = w.poll();
    if !ready.is_empty() {
        tempest_scripting::watch::reload(ready);
    }
    eval("tempest.reload('hot-reload'); tempest._with_mod('hot-reload', shared.hot_save, 'saved by mod'); return true");
    assert!(std::fs::read_to_string(hot.join("config.toml")).unwrap().contains("saved by mod"));
    std::thread::sleep(std::time::Duration::from_millis(80));
    assert!(w.poll().is_empty());
    std::thread::sleep(std::time::Duration::from_millis(80));
    assert!(w.poll().is_empty(), "the mod's own save must not reload it");
    // With reload_on_config off, a hand edit of config.toml is ignored too.
    let mut quiet = tempest_scripting::watch::Watcher::new(Some(game.join("mods")), Some(game.join("ChaosGame/Config/mods")), false, std::time::Duration::from_millis(50));
    std::fs::write(hot.join("config.toml"), "label = \"hand edit\"\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(80));
    assert!(quiet.poll().is_empty() && quiet.poll().is_empty());
    // An installed mod's user file lives in Config/mods and is watched as well.
    std::fs::create_dir_all(game.join("ChaosGame/Config/mods")).unwrap();
    assert!(w.poll().is_empty());
    std::fs::write(&hot_cfg, "label = \"user\"\n").unwrap();
    assert!(w.poll().is_empty());
    std::thread::sleep(std::time::Duration::from_millis(80));
    assert_eq!(w.poll(), vec!["hot-reload".to_string()]);
    let _ = std::fs::remove_dir_all(&game);

    // The shipped examples at least parse and run with nothing resolved.
    for src in [
        include_str!("../../../examples/00_status.lua"),
        include_str!("../../../examples/01_events.lua"),
        include_str!("../../../examples/02_post_login.lua"),
        include_str!("../../../examples/03_calls.lua"),
        include_str!("../../../examples/04_native_hook.lua"),
        include_str!("../../../mods/console/scripts/console.lua"),
        include_str!("../../../mods/scripting/scripts/core.lua"),
        include_str!("../../../examples/06_containers.lua"),
        include_str!("../../../examples/07_shared.lua"),
        include_str!("../../../examples/08_keys.lua"),
        include_str!("../../../examples/09_address_hook.lua"),
        include_str!("../../../examples/10_lua_natives.lua"),
        include_str!("../../../examples/11_ui.lua"),
    ] {
        lua.load(src).exec().expect("example runs on host");
    }
    assert!(unreal::clear_subscriptions().len() >= 6);
}
