//! The whole Lua surface must load and behave on a host with no game image:
//! every native lookup reports "unresolved" instead of panicking.

use tempest_scripting::unreal;

#[test]
fn layouts_match_the_sdk() {
    use unreal::layout::*;
    assert_eq!((UOBJECT_OUTER, UOBJECT_NAME, UOBJECT_CLASS), (0x40, 0x48, 0x50));
    assert_eq!((UFIELD_NEXT, USTRUCT_SUPER, USTRUCT_CHILDREN), (0x60, 0x78, 0x80));
    assert_eq!((UPROP_FLAGS, UPROP_OFFSET, UPROP_EXT), (0x70, 0x7C, 0x98));
    assert_eq!((CHEAT_ARRAY_COUNT, TARRAY_COUNT, FNAME_ENTRY_NAME), (0x0C, 0x08, 0x14));
    assert_eq!((CPF_PARM, CPF_OUT_PARM, CPF_RETURN_PARM), (0x80, 0x100, 0x400));
    assert_eq!((UFUNCTION_FUNC, UFUNCTION_SCRIPT_DATA, UFUNCTION_PARMS_SIZE), (0xF0, 0x8C, 0xE2));
    assert_eq!((FRAME_NODE, FRAME_OBJECT, FRAME_CODE, FRAME_LOCALS), (0x14, 0x1C, 0x24, 0x2C));
    for sig in [
        unreal::SIG_GOBJECTS,
        unreal::SIG_GNAMES,
        unreal::SIG_ENGINE,
        unreal::SIG_PROCESS_EVENT,
        unreal::SIG_XOR,
        unreal::SIG_GMALLOC,
        unreal::SIG_GNATIVES_LEA,
    ] {
        tempest_scripting::memory::parse_pattern(sig).unwrap();
    }
}

#[test]
fn host_smoke() {
    let g = unreal::resolve();
    assert!(!g.gnames_ok && g.process_event == 0 && !g.notes.is_empty());

    let lua = tempest_scripting::lua();
    let eval = |src: &str| lua.load(src).eval::<mlua::Value>().expect(src);

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
    std::env::set_var("TEMPEST_SCRIPT_DIR", concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/mods/tempest-scripting-lib/scripts"));
    tempest_scripting::loader::run_all(lua);
    std::env::remove_var("TEMPEST_SCRIPT_DIR");
    assert!(eval("return type(tempest.import('greet')) == 'function'").as_boolean().unwrap());
    assert!(eval("return LIB_PRIVATE == nil").as_boolean().unwrap());
    assert_eq!(eval("return shared.__providers.greet"), mlua::Value::String(lua.create_string("local").unwrap()));
    assert_eq!(eval("return #tempest.status().native_hooks"), mlua::Value::Integer(0));

    // Address hooks: refused on the host (no code to detour), bookkeeping intact.
    assert_eq!(eval("return pcall(memory.hook, 0x1000, function() end)"), mlua::Value::Boolean(false));
    assert_eq!(eval("return pcall(memory.hook, 0x1000, function() end, { args = 9 })"), mlua::Value::Boolean(false));
    assert_eq!(eval("return #memory.hooks() + #tempest.status().address_hooks"), mlua::Value::Integer(0));
    assert_eq!(eval("return memory.unhook(42)"), mlua::Value::Boolean(false));
    assert!(eval("return memory.call(0, 1, 2)").is_nil());
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
    assert_eq!(eval("return memory.write_str(0, 'x')"), mlua::Value::Boolean(false));

    // The shipped examples at least parse and run with nothing resolved.
    for src in [
        include_str!("../../../examples/00_status.lua"),
        include_str!("../../../examples/01_events.lua"),
        include_str!("../../../examples/02_post_login.lua"),
        include_str!("../../../examples/03_calls.lua"),
        include_str!("../../../examples/04_native_hook.lua"),
        include_str!("../../../packaging/console/scripts/console.lua"),
        include_str!("../../../packaging/scripting/scripts/core.lua"),
        include_str!("../../../examples/06_containers.lua"),
        include_str!("../../../examples/07_shared.lua"),
        include_str!("../../../examples/08_keys.lua"),
        include_str!("../../../examples/09_address_hook.lua"),
    ] {
        lua.load(src).exec().expect("example runs on host");
    }
    assert!(unreal::clear_subscriptions().len() >= 6);
}
