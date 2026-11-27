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
use tempest_assets::serve::{Server, TwrkSave};

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

/// A `twrk.save` whose editors were read, waiting to be written.
struct Save {
    job: TwrkSave,
    replies: Vec<oneshot::Sender<Result<Reply, String>>>,
}

/// The thread that writes workspace saves, one at a time. Saves of the same
/// file that queued up behind one being written are written once, from the
/// newest snapshot, which answers all of them.
fn start_saver() -> mpsc::Sender<Save> {
    let (tx, rx) = mpsc::channel::<Save>();
    thread::Builder::new()
        .name("twrk-saver".into())
        // Like the backend thread: a single delta is made on this one.
        .stack_size(64 << 20)
        .spawn(move || {
            let mut queued: Vec<Save> = Vec::new();
            loop {
                if queued.is_empty() {
                    match rx.recv() {
                        Ok(save) => queued.push(save),
                        Err(_) => break,
                    }
                }
                queued.extend(rx.try_iter());
                let out = queued[0].job.out().to_path_buf();
                let (same, rest): (Vec<Save>, Vec<Save>) = queued.into_iter().partition(|s| s.job.out() == out);
                queued = rest;
                let mut replies = Vec::new();
                let mut newest = None;
                for save in same {
                    replies.extend(save.replies);
                    newest = Some(save.job);
                }
                let Some(job) = newest else { continue };
                let result = job.run();
                for reply in replies {
                    let _ = reply.send(result.clone().map(|json| Reply { json, bins: Vec::new() }));
                }
            }
        })
        .expect("start the workspace saver thread");
    tx
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
                // Path-traced light bakes trace on the GPU when there is one.
                server.set_tracer_factory(crate::gpu_trace::factory());
                let saves = start_saver();
                for r in rx {
                    // A workspace save reads the editors here and is written on the
                    // saver thread, so the calls after it do not wait for the file.
                    if r.method == "twrk.save" {
                        match server.twrk_save_job(&r.params) {
                            Ok(job) => {
                                let _ = saves.send(Save { job, replies: vec![r.reply] });
                            }
                            Err(e) => {
                                let _ = r.reply.send(Err(e));
                            }
                        }
                        continue;
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Instant;

    /// Workspace saves are written on the saver thread (the calls after one
    /// do not wait for the file), saves of the same file sent together all
    /// answer with what was written, and the file holds the edit (a game
    /// package, when present; the output goes to a temporary folder).
    #[test]
    fn saves_are_written_off_the_backend_thread() {
        let Some(home) = std::env::var_os("HOME") else { return };
        let dir = std::path::Path::new(&home).join("Games/Tempest/8.1/ChaosGame/CookedPCConsole");
        let package = dir.join("DEVICE27309_LOBBY_ASSETS_SF.upk");
        if !package.exists() {
            eprintln!("skipped: no {}", package.display());
            return;
        }
        let out = std::env::temp_dir().join(format!("tempest-save-test-{}", std::process::id())).join("test.twrk");
        let (backend, _events) = Backend::start();
        let call = |method: &str, params: J| futures::executor::block_on(backend.call(method, params)).map(|r| r.json);
        call("workspace.open", json!({ "dir": dir })).unwrap();
        let path = package.to_string_lossy().to_string();
        call("package.open", json!({ "path": path })).unwrap();
        call("object.rename", json!({ "path": path, "index": 0, "name": "save_test_name" })).unwrap();
        let save = || backend.call("twrk.save", json!({ "out": out, "mode": "linked" }));
        let started = Instant::now();
        let first = save();
        let pinged = call("ping", json!({})).map(|_| started.elapsed());
        let (second, third) = (save(), save());
        let results: Vec<J> = futures::executor::block_on(futures::future::join_all([first, second, third])).into_iter().map(|r| r.unwrap().json).collect();
        eprintln!("ping answered after {:?}, three saves written in {:?}", pinged.unwrap(), started.elapsed());
        assert!(results.iter().all(|r| *r == results[0]));
        let read = call("twrk.read", json!({ "path": out })).unwrap();
        let files = read["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["stored"], "delta");
        let _ = std::fs::remove_dir_all(out.parent().unwrap());
    }
}
