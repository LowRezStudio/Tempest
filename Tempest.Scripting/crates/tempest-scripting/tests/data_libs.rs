//! `json`, `http` and `assembly` / `lang` through the Lua surface. Its own
//! test binary: draining the game-thread queue makes this thread the game's.
//!
//! The marshal half reads real game files when `TEMPEST_TEST_GAME` names a
//! game folder (links into a scratch folder, nothing is written there).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tempest_scripting::unreal;

/// A scratch game folder; with `real`, its `.dat` files and token DLL linked in.
fn fake_game_dir(real: Option<&Path>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tempest-scripting-data-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("ChaosGame/Config")).unwrap();
    std::fs::write(dir.join("ChaosGame/Config/tempest.toml"), "[dev]\nhot_reload = false\n").unwrap();
    if let Some(real) = real {
        for sub in ["ChaosGame/CookedPCConsole/assembly.dat", "ChaosGame/Localization/INT/Lang_INT.dat", "Binaries/Win64/MctsInterface.dll", "Binaries/Win32/MctsInterface.dll"] {
            if real.join(sub).is_file() {
                std::fs::create_dir_all(dir.join(sub).parent().unwrap()).unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(real.join(sub), dir.join(sub)).unwrap();
                #[cfg(not(unix))]
                std::fs::copy(real.join(sub), dir.join(sub)).unwrap();
            }
        }
    }
    dir
}

/// One-request HTTP server answering with what it received, as JSON.
fn echo_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(2) {
            let mut stream = stream.unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            r.read_line(&mut request_line).unwrap();
            let (mut len, mut ctype) = (0, String::new());
            loop {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                if l == "\r\n" {
                    break;
                }
                let (k, v) = l.split_once(':').unwrap();
                match k.to_ascii_lowercase().as_str() {
                    "content-length" => len = v.trim().parse().unwrap(),
                    "content-type" => ctype = v.trim().to_string(),
                    _ => {}
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let mut parts = request_line.split(' ');
            let reply = serde_json::json!({
                "method": parts.next(), "path": parts.next(), "type": ctype, "body": String::from_utf8(body).unwrap(),
            })
            .to_string();
            // Chunked, in two pieces, with a repeated header.
            let (a, b) = reply.split_at(reply.len() / 2);
            write!(stream, "HTTP/1.1 201 Created\r\nX-Tag: a\r\nX-Tag: b\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n", a.len(), b.len()).unwrap();
        }
    });
    format!("http://{addr}")
}

fn drain_until(lua: &mlua::Lua, done: &str) {
    let start = Instant::now();
    while !lua.load(done).eval::<bool>().unwrap() {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for {done}");
        unreal::drain_queue();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn data_libs() {
    let real = std::env::var_os("TEMPEST_TEST_GAME").map(PathBuf::from);
    let game = fake_game_dir(real.as_deref());
    std::env::set_var("TEMPEST_GAME_DIR", &game);
    let lua = tempest_scripting::lua();
    let run = |src: &str| lua.load(src).exec().unwrap_or_else(|e| panic!("{src}\n{e}"));

    // http: a JSON body, a query, a chunked reply, `res:json()`; then a cancelled request never answers.
    let url = echo_server();
    run(&format!(
        r#"http.post("{url}/items?x=1", {{ name = "zhin" }}, {{ query = {{ q = "a b" }} }}, function(res, err)
            assert(res, err)
            got = {{ status = res.status, ok = res.ok, tag = res.headers["x-tag"], echo = res:json() }}
        end)"#
    ));
    drain_until(lua, "return got ~= nil");
    run(r#"assert(got.status == 201 and got.ok and got.tag == "a, b", json.encode(got))
           assert(got.echo.method == "POST" and got.echo.path == "/items?x=1&q=a%20b", got.echo.path)
           assert(got.echo.type == "application/json" and json.decode(got.echo.body).name == "zhin")"#);
    run(&format!(r#"cancelled = http.get("{url}/", function() fired = true end) http.cancel(cancelled)"#));
    run(r#"http.get("http://127.0.0.1:1/", function(res, err) refused = err or "no error" end)"#);
    drain_until(lua, "return refused ~= nil");
    run("assert(not fired and refused:match('connect'), refused)");

    // A real HTTPS endpoint (WinHTTP's TLS), when `TEMPEST_TEST_HTTPS` names one.
    if let Ok(https) = std::env::var("TEMPEST_TEST_HTTPS") {
        run(&format!(r#"http.get("{https}", function(res, err) tls = res and res.status or err end)"#));
        drain_until(lua, "return tls ~= nil");
        run("assert(tls == 200, tls) print('https', tls)");
    }

    // The example registers its commands; captured here to run them below.
    run("handlers = {} real_command = tempest.command tempest.command = function(name, fn) handlers[name] = fn end");
    run(include_str!("../../../examples/12_data.lua"));
    run("tempest.command = real_command said = {} pc = { ClientMessage = function(_, m) said[#said + 1] = m end }");

    // Without the game files the lookups fail with the path.
    let Some(real) = real else {
        run("local ok, err = assembly.load() assert(ok == nil and err:match('assembly.dat'), err)");
        eprintln!("TEMPEST_TEST_GAME unset: assembly / lang not checked against game files");
        return;
    };
    let t = Instant::now();
    run("assert(assembly.load())");
    eprintln!("assembly.dat loaded in {} ms", t.elapsed().as_millis());
    run(r#"
        local sets = assembly.sets()
        assert(#sets > 10 and sets[1].name == "DATA_SET_ITEMS" and sets[1].rows > 1000)
        local items = assembly.rows("ITEMS")
        assert(#items == sets[1].rows)
        local first = items[1]
        local again = assembly.find("DATA_SET_ITEMS", "ITEM_ID", first.ITEM_ID)
        assert(again == first, tostring(again))
        assert(assembly.find("ITEMS", "ITEM_ID", -12345) == nil)
        assert(not pcall(function() return first.NOT_A_FIELD end))
        assert(first:has("ITEM_ID") and type(first:fields().ITEM_ID) == "number")
        local bot = assembly.find("BOTS", "BOT_TYPE_VALUE_ID", 10849)
        local devices = bot.DATA_SET_BOT_DEVICES
        assert(type(devices) == "table" and devices[1].DEVICE_ID, "bot devices")
        local same = assembly.find_all("DEVICES", { REF_BOT_ID = bot.BOT_ID })
        for _, d in ipairs(same) do assert(d.REF_BOT_ID == bot.BOT_ID) end
        assert(type(assembly.get("DB_VERSION")) == "string")
        print("assembly", assembly.version(), #items, "items;", bot.REFERENCE_NAME, #same, "devices")
    "#);
    if real.join("ChaosGame/Localization/INT/Lang_INT.dat").is_file() {
        run(r#"
            assert(lang.language() == "INT")
            local m = lang.rows()[1]
            assert(lang.message(m.MSG_ID) == m.MESSAGE)
            local bot = assembly.find("BOTS", "BOT_TYPE_VALUE_ID", 10849)
            print("lang", lang.message(bot.NAME_MSG_ID), lang.get("EXT_NAME"))
            assert(lang.message(-1) == nil)
            handlers.champion(pc, lang.message(bot.NAME_MSG_ID))
            assert(said[1]:match("bot " .. bot.BOT_ID), said[1])
            print(table.concat(said, "\n"))
        "#);
    }
    run("assert(assembly.unload() and not assembly.unload())");
}
