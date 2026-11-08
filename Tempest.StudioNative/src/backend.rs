//! The asset backend, in-process: the same `upk serve` server the Electron
//! Studio talks to over stdio, run on a thread of its own (packages, the
//! class database and the undo journals live there) and called with
//! messages. Replies carry their binary payloads as bytes, events (search,
//! bake, map progress) go to a channel the UI drains.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use futures::channel::oneshot;
use serde_json::Value as J;
use tempest_assets::serve::Server;

/// A reply: the result JSON and the payloads its `{"$bin": n}` markers name.
pub struct Reply {
    pub json: J,
    pub bins: Vec<Vec<u8>>,
}

impl Reply {
    /// The payload a `{"$bin": n}` marker at `v` names.
    pub fn bin(&self, v: &J) -> Option<&[u8]> {
        let n = v.get("$bin")?.as_u64()? as usize;
        self.bins.get(n).map(|b| b.as_slice())
    }
}

/// Work in flight (backend calls, background builds): the test driver waits
/// for it to reach zero before a step counts as done.
pub static WORK: AtomicUsize = AtomicUsize::new(0);

/// Counts the calls that change something (an edit, an undo, a save): a
/// project saves itself once this moves.
pub static EDITS: AtomicUsize = AtomicUsize::new(0);

fn is_edit(method: &str) -> bool {
    let verb = method.rsplit('.').next().unwrap_or(method);
    const VERBS: &[&str] = &["set", "add", "remove", "delete", "rename", "duplicate", "copy", "replace", "import", "create", "undo", "discard", "apply", "new", "own", "insert", "edit", "link", "save", "findReplace", "vendorAdd", "collectionAdd"];
    VERBS.iter().any(|v| verb.starts_with(v)) && method != "twrk.save" || matches!(method, "batch.run" | "script.compile" | "sound.replacePcm" | "wwise.replacePcm")
}

/// Counts as work while alive.
pub struct Busy;

impl Busy {
    pub fn new() -> Busy {
        WORK.fetch_add(1, Ordering::SeqCst);
        Busy
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        WORK.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Request {
    method: String,
    params: J,
    reply: oneshot::Sender<Result<Reply, String>>,
}

#[derive(Clone)]
pub struct Backend {
    tx: mpsc::Sender<Request>,
}

impl Backend {
    /// Start the server thread; its events arrive on the returned receiver.
    pub fn start() -> (Backend, futures::channel::mpsc::UnboundedReceiver<J>) {
        let (tx, rx) = mpsc::channel::<Request>();
        let (etx, erx) = futures::channel::mpsc::unbounded::<J>();
        thread::Builder::new()
            .name("upk-server".into())
            // Deep object graphs (copies, Kismet) recurse.
            .stack_size(64 << 20)
            .spawn(move || {
                let mut server = Server::with_events(move |e| {
                    let _ = etx.unbounded_send(e);
                });
                for r in rx {
                    let out = server.call_bin(&r.method, &r.params).map(|(json, bins)| Reply { json, bins });
                    let _ = r.reply.send(out);
                }
            })
            .expect("start the backend thread");
        (Backend { tx }, erx)
    }

    /// Call a backend method (`serve.rs`); requests run one at a time, in order.
    pub fn call(&self, method: &str, params: J) -> impl std::future::Future<Output = Result<Reply, String>> + use<> {
        let (reply, rx) = oneshot::channel();
        let busy = Busy::new();
        let edit = is_edit(method);
        let sent = self.tx.send(Request { method: method.to_string(), params, reply });
        async move {
            let _busy = busy;
            sent.map_err(|_| "the backend has stopped".to_string())?;
            let r = rx.await.map_err(|_| "the backend has stopped".to_string())?;
            if edit && r.is_ok() {
                EDITS.fetch_add(1, Ordering::SeqCst);
            }
            r
        }
    }
}
