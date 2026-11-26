//! `http.*`: HTTP(S) requests off the game thread, answered on it.
//!
//! ```lua
//! http.get("https://example.com/api/items", function(res, err)
//!     if not res then return print("failed: " .. err) end
//!     print(res.status, res.headers["content-type"], #res.body)
//!     local data = res:json()
//! end)
//! http.post(url, { name = "x" }, function(res, err) end)  -- a table body is sent as JSON
//! http.request({ url = url, method = "PUT", headers = { Authorization = "Bearer ..." },
//!     query = { page = 2 }, body = "raw", timeout = 10 }, function(res, err) end)
//! ```
//!
//! Each request runs on its own thread; the callback runs on the game thread
//! at the next event, attributed to the mod that asked. `res` is
//! `{ status, ok (2xx), headers (lower-case names), body, url }`; `err` is
//! set (and `res` nil) only when no response came back (DNS, connect, TLS,
//! timeout). Reloading a mod drops the callbacks it still had pending.
//!
//! Windows goes through WinHTTP: the system's TLS, certificate store and
//! proxy settings, redirects followed, gzip/deflate decoded. The host build
//! (tests) has a plain-HTTP/1.1 client over `TcpStream` with no TLS.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use mlua::{Function, Lua, Table, Value};

/// Responses larger than this fail instead of filling the address space of the Win32 client.
const MAX_BODY: usize = 256 << 20;

pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timeout: Duration,
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// The parts of an `http://` or `https://` URL a connection needs.
#[derive(Debug, PartialEq)]
pub struct Url {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    /// Path and query, starting with `/`.
    pub path: String,
}

pub fn parse_url(url: &str) -> Result<Url, String> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| format!("{url:?} is not an absolute URL"))?;
    let secure = match scheme.to_ascii_lowercase().as_str() {
        "http" => false,
        "https" => true,
        s => return Err(format!("unsupported scheme {s:?} (http and https only)")),
    };
    let rest = rest.split('#').next().unwrap_or_default();
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) if rest[i..].starts_with('?') => (&rest[..i], format!("/{}", &rest[i..])),
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = if let Some(v6) = host_port.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or("unterminated IPv6 address")?;
        (h, after.strip_prefix(':'))
    } else {
        match host_port.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (host_port, None),
        }
    };
    if host.is_empty() {
        return Err(format!("{url:?} has no host"));
    }
    let port = match port {
        Some(p) => p.parse().map_err(|_| format!("bad port {p:?}"))?,
        None if secure => 443,
        None => 80,
    };
    Ok(Url { secure, host: host.to_string(), port, path })
}

/// Percent-encoding for query strings (RFC 3986 unreserved characters kept).
pub fn urlencode(s: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ----------------------------------------------------------------------------
// Pending callbacks
// ----------------------------------------------------------------------------

struct Pending {
    owner: String,
    callback: mlua::RegistryKey,
}

static PENDING: Mutex<Option<HashMap<u64, Pending>>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Drop the callbacks a mod still waits on (every mod's when `None`): its
/// environment is about to be replaced. The requests finish unanswered.
pub fn forget(lua: &Lua, owner: Option<&str>) {
    let dropped: Vec<Pending> = {
        let mut pending = PENDING.lock().unwrap();
        let Some(map) = pending.as_mut() else { return };
        let ids: Vec<u64> = map.iter().filter(|(_, p)| owner.is_none_or(|o| p.owner.eq_ignore_ascii_case(o))).map(|(id, _)| *id).collect();
        ids.into_iter().filter_map(|id| map.remove(&id)).collect()
    };
    for p in dropped {
        let _ = lua.remove_registry_value(p.callback);
    }
}

fn start(lua: &Lua, req: Request, callback: Function) -> mlua::Result<u64> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let owner = crate::loader::current_mod();
    let label = format!("http {} {}", req.method, req.url);
    PENDING.lock().unwrap().get_or_insert_default().insert(id, Pending { owner, callback: lua.create_registry_value(callback)? });
    let spawned = std::thread::Builder::new().name("tempest-http".into()).spawn(move || {
        let result = send(&req);
        crate::unreal::queue(Box::new(move || answer(id, &req.url, label, result)));
    });
    if let Err(e) = spawned {
        forget_one(lua, id);
        return Err(mlua::Error::external(format!("http: cannot start a thread: {e}")));
    }
    Ok(id)
}

fn forget_one(lua: &Lua, id: u64) {
    let p = PENDING.lock().unwrap().as_mut().and_then(|m| m.remove(&id));
    if let Some(p) = p {
        let _ = lua.remove_registry_value(p.callback);
    }
}

/// On the game thread: hand the result to the callback, unless its mod was reloaded meanwhile.
fn answer(id: u64, url: &str, label: String, result: Result<Response, String>) {
    let Some(p) = PENDING.lock().unwrap().as_mut().and_then(|m| m.remove(&id)) else { return };
    let lua = crate::lua();
    let run = || -> mlua::Result<()> {
        let f: Function = lua.registry_value(&p.callback)?;
        match result {
            Ok(res) => f.call((response_table(lua, url, res)?, Value::Nil)),
            Err(e) => f.call((Value::Nil, e)),
        }
    };
    if let Err(e) = crate::loader::with_mod_at(&p.owner, crate::profile::Site::Named("http", label.clone()), || label.clone(), run) {
        crate::log::line("lua", &format!("[{}] {label}: callback failed: {e}", p.owner));
    }
    let _ = lua.remove_registry_value(p.callback);
}

fn response_table(lua: &Lua, url: &str, res: Response) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("status", res.status)?;
    t.set("ok", (200..300).contains(&res.status))?;
    t.set("url", url)?;
    let headers = lua.create_table()?;
    for (k, v) in res.headers {
        let k = k.to_ascii_lowercase();
        let v = match headers.get::<Option<String>>(k.as_str())? {
            Some(prev) => format!("{prev}, {v}"),
            None => v,
        };
        headers.set(k, v)?;
    }
    t.set("headers", headers)?;
    t.set("body", lua.create_string(&res.body)?)?;
    t.set("json", lua.create_function(|lua, this: Table| crate::json::decode(lua, &this.get::<mlua::String>("body")?.as_bytes()))?)?;
    Ok(t)
}

// ----------------------------------------------------------------------------
// Lua
// ----------------------------------------------------------------------------

/// A request from `http.request`'s options table.
fn request_from(lua: &Lua, opts: &Table) -> mlua::Result<Request> {
    let mut url: String = opts.get("url").map_err(|_| mlua::Error::external("http.request: `url` is required"))?;
    if let Some(query) = opts.get::<Option<Table>>("query")? {
        let mut parts = Vec::new();
        for pair in query.pairs::<mlua::String, mlua::String>() {
            let (k, v) = pair?;
            parts.push(format!("{}={}", urlencode(&k.as_bytes()), urlencode(&v.as_bytes())));
        }
        parts.sort();
        if !parts.is_empty() {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&parts.join("&"));
        }
    }
    let mut headers = Vec::new();
    if let Some(h) = opts.get::<Option<Table>>("headers")? {
        for pair in h.pairs::<String, String>() {
            let (k, v) = pair?;
            if k.contains(['\r', '\n', ':']) || v.contains(['\r', '\n']) {
                return Err(mlua::Error::external(format!("http.request: bad header {k:?}")));
            }
            headers.push((k, v));
        }
    }
    let has = |headers: &[(String, String)], name: &str| headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name));
    let body = match opts.get::<Value>("body")? {
        Value::Nil => Vec::new(),
        Value::String(s) => s.as_bytes().to_vec(),
        v @ Value::Table(_) => {
            if !has(&headers, "content-type") {
                headers.push(("Content-Type".into(), "application/json".into()));
            }
            serde_json::to_vec(&crate::json::to_json(lua, &v)?).map_err(mlua::Error::external)?
        }
        other => return Err(mlua::Error::external(format!("http.request: `body` must be a string or a table, not a {}", other.type_name()))),
    };
    if !has(&headers, "user-agent") {
        headers.push(("User-Agent".into(), format!("Tempest.Scripting/{}", env!("CARGO_PKG_VERSION"))));
    }
    let method = opts.get::<Option<String>>("method")?.unwrap_or_else(|| if body.is_empty() { "GET" } else { "POST" }.into()).to_ascii_uppercase();
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_alphabetic()) {
        return Err(mlua::Error::external(format!("http.request: bad method {method:?}")));
    }
    let timeout = opts.get::<Option<f64>>("timeout")?.unwrap_or(30.0);
    if !(timeout > 0.0 && timeout < 86_400.0) {
        return Err(mlua::Error::external("http.request: `timeout` is in seconds, above 0"));
    }
    parse_url(&url).map_err(|e| mlua::Error::external(format!("http.request: {e}")))?;
    Ok(Request { method, url, headers, body, timeout: Duration::from_secs_f64(timeout) })
}

pub fn register(lua: &Lua) -> mlua::Result<()> {
    let t = lua.create_table()?;
    t.set("request", lua.create_function(|lua, (opts, cb): (Table, Function)| start(lua, request_from(lua, &opts)?, cb))?)?;
    // http.get(url, cb) / http.get(url, opts, cb)
    t.set(
        "get",
        lua.create_function(|lua, (url, a, b): (String, Value, Option<Function>)| {
            let (opts, cb) = match (a, b) {
                (Value::Function(cb), None) => (lua.create_table()?, cb),
                (Value::Table(opts), Some(cb)) => (opts, cb),
                _ => return Err(mlua::Error::external("usage: http.get(url, [opts], callback)")),
            };
            opts.set("url", url)?;
            opts.set("method", "GET")?;
            start(lua, request_from(lua, &opts)?, cb)
        })?,
    )?;
    // http.post(url, body, cb) / http.post(url, body, opts, cb)
    t.set(
        "post",
        lua.create_function(|lua, (url, body, a, b): (String, Value, Value, Option<Function>)| {
            let (opts, cb) = match (a, b) {
                (Value::Function(cb), None) => (lua.create_table()?, cb),
                (Value::Table(opts), Some(cb)) => (opts, cb),
                _ => return Err(mlua::Error::external("usage: http.post(url, body, [opts], callback)")),
            };
            opts.set("url", url)?;
            opts.set("method", "POST")?;
            opts.set("body", body)?;
            start(lua, request_from(lua, &opts)?, cb)
        })?,
    )?;
    t.set(
        "cancel",
        lua.create_function(|lua, id: u64| {
            forget_one(lua, id);
            Ok(())
        })?,
    )?;
    t.set("urlencode", lua.create_function(|_, s: mlua::String| Ok(urlencode(&s.as_bytes())))?)?;
    lua.globals().set("http", t)
}

// ----------------------------------------------------------------------------
// Transport
// ----------------------------------------------------------------------------

#[cfg(windows)]
pub use winhttp::send;

#[cfg(not(windows))]
pub use plain::send;

#[cfg(windows)]
mod winhttp {
    use super::*;
    use std::ffi::c_void;

    type Handle = *mut c_void;

    const ACCESS_TYPE_DEFAULT_PROXY: u32 = 0;
    /// Windows 8.1+: the system's proxy settings and auto-config.
    const ACCESS_TYPE_AUTOMATIC_PROXY: u32 = 4;
    const FLAG_SECURE: u32 = 0x0080_0000;
    const OPTION_DECOMPRESSION: u32 = 118;
    const DECOMPRESSION_FLAG_ALL: u32 = 3;
    const QUERY_STATUS_CODE: u32 = 19;
    const QUERY_RAW_HEADERS_CRLF: u32 = 22;
    const QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    #[link(name = "winhttp")]
    extern "system" {
        fn WinHttpOpen(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> Handle;
        fn WinHttpConnect(session: Handle, server: *const u16, port: u16, reserved: u32) -> Handle;
        fn WinHttpOpenRequest(connect: Handle, verb: *const u16, object: *const u16, version: *const u16, referrer: *const u16, accept: *const *const u16, flags: u32) -> Handle;
        fn WinHttpSetTimeouts(h: Handle, resolve: i32, connect: i32, send: i32, receive: i32) -> i32;
        fn WinHttpSetOption(h: Handle, option: u32, buffer: *const c_void, len: u32) -> i32;
        fn WinHttpSendRequest(h: Handle, headers: *const u16, headers_len: u32, optional: *const c_void, optional_len: u32, total_len: u32, context: usize) -> i32;
        fn WinHttpReceiveResponse(h: Handle, reserved: *mut c_void) -> i32;
        fn WinHttpQueryHeaders(h: Handle, level: u32, name: *const u16, buffer: *mut c_void, len: *mut u32, index: *mut u32) -> i32;
        fn WinHttpQueryDataAvailable(h: Handle, available: *mut u32) -> i32;
        fn WinHttpReadData(h: Handle, buffer: *mut c_void, len: u32, read: *mut u32) -> i32;
        fn WinHttpCloseHandle(h: Handle) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetLastError() -> u32;
    }

    struct Owned(Handle);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: a handle WinHTTP returned, closed once.
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn fail(what: &str) -> String {
        // SAFETY: plain FFI call.
        let code = unsafe { GetLastError() };
        let why = match code {
            12002 => "timed out",
            12005 | 12006 => "bad URL",
            12007 => "host name not resolved",
            12029 => "cannot connect",
            12030 | 12152 => "connection closed",
            12038 | 12044 | 12045 | 12169 | 12175 => "TLS / certificate failure",
            _ => "",
        };
        if why.is_empty() { format!("{what} failed (WinHTTP error {code})") } else { format!("{what} failed: {why} (WinHTTP error {code})") }
    }

    fn handle(h: Handle, what: &str) -> Result<Owned, String> {
        if h.is_null() { Err(fail(what)) } else { Ok(Owned(h)) }
    }

    fn session() -> Result<Owned, String> {
        let agent = wide(&format!("Tempest.Scripting/{}", env!("CARGO_PKG_VERSION")));
        // SAFETY: valid NUL-terminated strings; null proxy names.
        let h = unsafe { WinHttpOpen(agent.as_ptr(), ACCESS_TYPE_AUTOMATIC_PROXY, std::ptr::null(), std::ptr::null(), 0) };
        if !h.is_null() {
            return Ok(Owned(h));
        }
        // Windows 7 has no automatic proxy access type.
        handle(unsafe { WinHttpOpen(agent.as_ptr(), ACCESS_TYPE_DEFAULT_PROXY, std::ptr::null(), std::ptr::null(), 0) }, "WinHttpOpen")
    }

    pub fn send(req: &Request) -> Result<Response, String> {
        let url = parse_url(&req.url)?;
        let session = session()?;
        let ms = req.timeout.as_millis().min(i32::MAX as u128) as i32;
        let (host, verb, path) = (wide(&url.host), wide(&req.method), wide(&url.path));
        let mut headers = String::new();
        for (k, v) in &req.headers {
            headers.push_str(&format!("{k}: {v}\r\n"));
        }
        let headers = wide(&headers);
        // SAFETY: every pointer is a live NUL-terminated buffer or null; the
        // handles are closed by `Owned` after their children.
        unsafe {
            WinHttpSetTimeouts(session.0, ms, ms, ms, ms);
            let connect = handle(WinHttpConnect(session.0, host.as_ptr(), url.port, 0), "connect")?;
            let flags = if url.secure { FLAG_SECURE } else { 0 };
            let request = handle(WinHttpOpenRequest(connect.0, verb.as_ptr(), path.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null(), flags), "open request")?;
            // Windows 8.1+; older systems get the body as sent.
            let decompress = DECOMPRESSION_FLAG_ALL;
            WinHttpSetOption(request.0, OPTION_DECOMPRESSION, &decompress as *const u32 as *const c_void, 4);
            let body = if req.body.is_empty() { std::ptr::null() } else { req.body.as_ptr() as *const c_void };
            let len = req.body.len() as u32;
            if WinHttpSendRequest(request.0, headers.as_ptr(), u32::MAX, body, len, len, 0) == 0 {
                return Err(fail("send"));
            }
            if WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0 {
                return Err(fail("receive"));
            }
            let mut status = 0u32;
            let mut size = 4u32;
            let flag = QUERY_STATUS_CODE | QUERY_FLAG_NUMBER;
            if WinHttpQueryHeaders(request.0, flag, std::ptr::null(), &mut status as *mut u32 as *mut c_void, &mut size, std::ptr::null_mut()) == 0 {
                return Err(fail("status"));
            }
            let mut size = 0u32;
            WinHttpQueryHeaders(request.0, QUERY_RAW_HEADERS_CRLF, std::ptr::null(), std::ptr::null_mut(), &mut size, std::ptr::null_mut());
            let mut raw = vec![0u16; size as usize / 2 + 1];
            let headers = if GetLastError() == ERROR_INSUFFICIENT_BUFFER
                && WinHttpQueryHeaders(request.0, QUERY_RAW_HEADERS_CRLF, std::ptr::null(), raw.as_mut_ptr() as *mut c_void, &mut size, std::ptr::null_mut()) != 0
            {
                raw.truncate(size as usize / 2);
                parse_headers(&String::from_utf16_lossy(&raw))
            } else {
                Vec::new()
            };
            let mut body = Vec::new();
            loop {
                let mut available = 0u32;
                if WinHttpQueryDataAvailable(request.0, &mut available) == 0 {
                    return Err(fail("read"));
                }
                if available == 0 {
                    break;
                }
                if body.len() + available as usize > MAX_BODY {
                    return Err(format!("response larger than {} MB", MAX_BODY >> 20));
                }
                let at = body.len();
                body.resize(at + available as usize, 0);
                let mut read = 0u32;
                if WinHttpReadData(request.0, body[at..].as_mut_ptr() as *mut c_void, available, &mut read) == 0 {
                    return Err(fail("read"));
                }
                body.truncate(at + read as usize);
            }
            Ok(Response { status: status as u16, headers, body })
        }
    }
}

/// `Name: value` lines after the status line.
fn parse_headers(raw: &str) -> Vec<(String, String)> {
    raw.split("\r\n").skip(1).filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}

#[cfg(not(windows))]
mod plain {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};

    pub fn send(req: &Request) -> Result<Response, String> {
        let url = parse_url(&req.url)?;
        if url.secure {
            return Err("https is only available in the Windows build".into());
        }
        let addr = (url.host.as_str(), url.port).to_socket_addrs().map_err(|e| format!("{}: {e}", url.host))?.next().ok_or("host name not resolved")?;
        let mut stream = TcpStream::connect_timeout(&addr, req.timeout).map_err(|e| format!("connect: {e}"))?;
        stream.set_read_timeout(Some(req.timeout)).and_then(|_| stream.set_write_timeout(Some(req.timeout))).map_err(|e| e.to_string())?;
        let mut head = format!("{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n", req.method, url.path, url.host, req.body.len());
        for (k, v) in &req.headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&req.body)).map_err(|e| format!("send: {e}"))?;
        let mut r = BufReader::new(stream);
        let mut raw = String::new();
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).map_err(|e| format!("receive: {e}"))? == 0 || line == "\r\n" {
                break;
            }
            raw.push_str(&line);
        }
        let status = raw.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or("bad status line")?;
        let headers = parse_headers(&raw);
        let header = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
        let mut body = Vec::new();
        if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
            loop {
                let mut size = String::new();
                r.read_line(&mut size).map_err(|e| e.to_string())?;
                let n = usize::from_str_radix(size.trim().split(';').next().unwrap_or_default(), 16).map_err(|_| "bad chunk size")?;
                if n == 0 {
                    break;
                }
                if body.len() + n > MAX_BODY {
                    return Err(format!("response larger than {} MB", MAX_BODY >> 20));
                }
                let at = body.len();
                body.resize(at + n + 2, 0);
                r.read_exact(&mut body[at..]).map_err(|e| e.to_string())?;
                body.truncate(at + n);
            }
        } else {
            r.take(MAX_BODY as u64 + 1).read_to_end(&mut body).map_err(|e| format!("receive: {e}"))?;
            if body.len() > MAX_BODY {
                return Err(format!("response larger than {} MB", MAX_BODY >> 20));
            }
        }
        Ok(Response { status, headers, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(parse_url("https://example.com").unwrap(), Url { secure: true, host: "example.com".into(), port: 443, path: "/".into() });
        assert_eq!(parse_url("http://u:p@[::1]:8080/a/b?x=1#f").unwrap(), Url { secure: false, host: "::1".into(), port: 8080, path: "/a/b?x=1".into() });
        assert_eq!(parse_url("http://h?q").unwrap().path, "/?q");
        assert!(parse_url("ftp://h").is_err());
        assert!(parse_url("example.com").is_err());
        assert_eq!(urlencode("a b&é".as_bytes()), "a%20b%26%C3%A9");
    }
}
