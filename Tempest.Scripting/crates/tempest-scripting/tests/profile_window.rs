//! The profiler through the Lua API, and the performance window of the
//! runtime mod drawn in a headless UI frame.
use tempest_scripting::unreal;

#[test]
fn profile_reports_mods_and_the_window_draws() {
    let game = std::env::temp_dir().join(format!("tempest-scripting-profile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&game);
    std::fs::create_dir_all(game.join("ChaosGame/Config")).unwrap();
    std::fs::write(game.join("ChaosGame/Config/tempest.toml"), "[dev]\nhot_reload = false\n").unwrap();
    std::env::set_var("TEMPEST_GAME_DIR", &game);
    let _ = unreal::resolve();

    let lua = tempest_scripting::lua();
    let eval = |src: &str| lua.load(src).eval::<mlua::Value>().expect(src);
    assert!(eval("return tempest._ui_headless(1280, 720)").as_boolean().unwrap());

    // The runtime mod's scripts (core.lua: the commands and the window).
    std::env::set_var("TEMPEST_SCRIPT_DIR", concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/scripting/scripts"));
    tempest_scripting::loader::run_all(lua);
    std::env::remove_var("TEMPEST_SCRIPT_DIR");
    // Handler failures are only logged; keep what is logged from here on.
    eval("_G.logged = {} local log = tempest.log; tempest.log = function(s) table.insert(_G.logged, s); log(s) end; return true");

    assert_eq!(eval("return tempest.profile().on"), mlua::Value::Boolean(false));
    eval("tempest.profile(true); return true");
    eval("tempest._run_as('spinner', 'command', 'spin', function() local t = os.clock(); while os.clock() - t < 0.003 do end end); return true");
    eval("tempest._with_mod('drawer', function() ui.on_draw(function() local t = os.clock(); while os.clock() - t < 0.002 do end end) end); return true");
    for _ in 0..3 {
        assert!(eval("return tempest._ui_build_frame()").as_boolean().unwrap());
    }
    eval(
        "local r = tempest.profile() \
         assert(r.on and r.frames >= 2, 'frames ' .. r.frames) \
         local by = {} for _, m in ipairs(r.mods) do by[m.id] = m end \
         assert(by.spinner and by.spinner.ms >= 1.9, 'spinner ' .. tostring(by.spinner and by.spinner.ms)) \
         assert(by.spinner.sites[1].kind == 'command' and by.spinner.sites[1].label == 'spin') \
         local draw = by.drawer and by.drawer.sites[1] \
         assert(draw and draw.kind == 'draw' and draw.calls == 3 and draw.label:find('^ui.on_draw #'), 'drawer') \
         assert(by.drawer.worst_frame_ms >= 1.5 and by.drawer.worst_frame_ms < 50, 'worst ' .. by.drawer.worst_frame_ms) \
         local ui_frame for _, rt in ipairs(r.runtime) do if rt.name:find('UI frame') then ui_frame = rt end end \
         assert(ui_frame and ui_frame.calls == 3, 'ui frame') \
         return true",
    );

    // Ctrl+Shift+P opens the window (the binding core.lua makes), which draws a few frames.
    eval("tempest._key(0x50, true, true, false, true, false); return true");
    for _ in 0..3 {
        assert!(eval("return tempest._ui_build_frame()").as_boolean().unwrap());
    }
    assert!(eval("return tempest.profile().on").as_boolean().unwrap());
    eval("tempest._key(0x50, true, true, false, true, false); return true");
    assert_eq!(eval("return tempest.profile().on"), mlua::Value::Boolean(false));

    // Typed into the console window: commands run, everything else is Lua.
    assert!(tempest_scripting::lua_api::console_command(lua, "tempest_perf log").unwrap());
    assert!(!tempest_scripting::lua_api::console_command(lua, "x_from_console = 1").unwrap());
    assert!(eval("for _, s in ipairs(_G.logged) do if s:find('^%[perf%] measured') then return true end end return false").as_boolean().unwrap());
    // The window's own cost is named, not counted with the mods.
    assert!(eval("for _, s in ipairs(_G.logged) do if s:find('performance window (not in the totals)', 1, true) then return true end end return false").as_boolean().unwrap());

    let logged = eval("return table.concat(_G.logged, '\\n')");
    let logged = logged.as_string().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    assert!(!logged.contains("failed"), "{logged}");
    // The window's own draw handler ran: it is in the report as the runtime mod's draw entry.
    assert!(eval("for _, m in ipairs(tempest.profile().mods) do for _, s in ipairs(m.sites) do if s.kind == 'draw' and m.id ~= 'drawer' and s.calls > 0 then return true end end end return false").as_boolean().unwrap());
}
