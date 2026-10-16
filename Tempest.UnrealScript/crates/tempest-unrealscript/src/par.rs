//! Minimal fork/join helpers over `std::thread::scope`: a parallel map and a
//! dependency-ordered scheduler, so that per-class decompilation and per-package
//! builds spread across cores without pulling in a thread-pool dependency.
//!
//! The compiler itself stays single-threaded: name interning and import creation
//! order determine the bytes it emits, so only work that shares nothing but an
//! immutable [`crate::context::Context`] (or separate contexts) runs in parallel.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

static JOBS: AtomicUsize = AtomicUsize::new(0);

/// Set the number of worker threads (`-j N`); 0 restores the default.
pub fn set_jobs(n: usize) {
    JOBS.store(n, Ordering::Relaxed);
}

/// Worker threads to use: `-j`, else `USCRIPT_JOBS`, else the available parallelism.
pub fn jobs() -> usize {
    let n = JOBS.load(Ordering::Relaxed);
    if n > 0 {
        return n;
    }
    if let Some(n) = std::env::var("USCRIPT_JOBS").ok().and_then(|v| v.parse::<usize>().ok()).filter(|&n| n > 0) {
        return n;
    }
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// `items.iter().map(f).collect()`, with the calls spread over [`jobs`] threads.
/// Results come back in input order; `f` runs on the calling thread when there is
/// nothing to gain from spawning.
pub fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = jobs().min(items.len());
    if threads <= 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let mut slots: Vec<Option<R>> = Vec::with_capacity(items.len());
    slots.resize_with(items.len(), || None);
    let slots = Mutex::new(slots);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                // Work in bursts so a long-running item never blocks the others
                // while results of the burst are being stored.
                let mut done: Vec<(usize, R)> = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= items.len() {
                        break;
                    }
                    done.push((i, f(&items[i])));
                    if done.len() >= 16 {
                        let mut g = slots.lock().unwrap();
                        for (i, r) in done.drain(..) {
                            g[i] = Some(r);
                        }
                    }
                }
                let mut g = slots.lock().unwrap();
                for (i, r) in done {
                    g[i] = Some(r);
                }
            });
        }
    });
    slots.into_inner().unwrap().into_iter().map(|r| r.expect("every item produced a result")).collect()
}

/// Run `f(i)` for every task once all of its `deps` (indices into `tasks`) have
/// finished, at most [`jobs`] tasks at a time. Tasks are started in index order
/// among those ready. The first error stops further scheduling; tasks already
/// running complete. Returns the errors in task order.
pub fn schedule<E: Send>(deps: &[Vec<usize>], f: impl Fn(usize) -> Result<(), E> + Sync) -> Vec<(usize, E)> {
    #[derive(Clone, Copy, PartialEq)]
    enum St {
        Waiting,
        Running,
        Done,
    }
    struct State<E> {
        st: Vec<St>,
        errors: Vec<(usize, E)>,
        failed: bool,
    }
    let n = deps.len();
    let threads = jobs().min(n);
    let state = Mutex::new(State { st: vec![St::Waiting; n], errors: Vec::new(), failed: false });
    let cv = Condvar::new();
    let f = &f;
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| {
                let mut g = state.lock().unwrap();
                loop {
                    if g.failed {
                        break;
                    }
                    let ready = (0..n).find(|&i| g.st[i] == St::Waiting && deps[i].iter().all(|&d| g.st[d] == St::Done));
                    match ready {
                        Some(i) => {
                            g.st[i] = St::Running;
                            drop(g);
                            let r = f(i);
                            g = state.lock().unwrap();
                            g.st[i] = St::Done;
                            if let Err(e) = r {
                                g.errors.push((i, e));
                                g.failed = true;
                            }
                            cv.notify_all();
                        }
                        None => {
                            if !g.st.contains(&St::Running) {
                                // Finished, or nothing can ever become ready (a cycle).
                                break;
                            }
                            g = cv.wait(g).unwrap();
                        }
                    }
                }
            });
        }
    });
    let mut errors = state.into_inner().unwrap().errors;
    errors.sort_by_key(|(i, _)| *i);
    errors
}
