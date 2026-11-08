//! Movies are drawn by Ruffle (its Rust core and wgpu renderer, from
//! `vendor/ruffle`, `scripts/fetch-ruffle.sh`), off-screen, on a thread of
//! its own: a player is not `Send`, and making one takes a second or more
//! (ActionScript globals, parsing). The backend prepares what Ruffle reads,
//! as for the Electron Studio's: `swf.preview` cuts a layout or a character
//! with its own cuts of the shared libraries, `swf.player` and
//! `swf.libraries` give the whole movie and libraries, the package's
//! textures embedded as bitmaps. Libraries are written to a temporary
//! folder the movie's `ImportAssets2` URLs resolve against.
//!
//! Two kinds of work: a still (build a player, run a few frames so imports
//! load and the first picture settles, capture, drop it) and one live player
//! (Play: advanced a frame at a time, mouse events passed on).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use futures::channel::oneshot;
use image::RgbaImage;
use ruffle_core::backend::navigator::{NullExecutor, NullNavigatorBackend};
use ruffle_core::events::{MouseButton, PlayerEvent};
use ruffle_core::limits::ExecutionLimit;
use ruffle_core::tag_utils::SwfMovie;
use ruffle_core::{Player, PlayerBuilder, ViewportDimensions};
use ruffle_render_wgpu::backend::{create_wgpu_instance, request_adapter_and_device, WgpuRenderBackend};
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_render_wgpu::target::TextureTarget;
use ruffle_render_wgpu::wgpu;

/// A movie to play and the libraries its imports name (URL as the movie spells it, bytes).
pub struct Load {
    pub swf: Vec<u8>,
    pub libraries: Vec<(String, Vec<u8>)>,
    /// The picture's size in pixels; the movie is scaled to fill it.
    pub width: u32,
    pub height: u32,
    /// No background: what is around the drawing stays transparent.
    pub transparent: bool,
}

/// What a live player reports when it starts.
#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub frame_rate: f64,
    pub width: f64,
    pub height: f64,
}

/// A pointer event on the live player, in picture pixels.
#[derive(Clone, Copy, Debug)]
pub enum Pointer {
    Move(f64, f64),
    Down(f64, f64),
    Up(f64, f64),
    Leave,
}

enum Job {
    Still { load: Load, frames: u32, reply: oneshot::Sender<Result<RgbaImage, String>> },
    Start { load: Load, reply: oneshot::Sender<Result<Info, String>> },
    Advance { frames: u32, size: Option<(u32, u32)>, reply: oneshot::Sender<Result<RgbaImage, String>> },
    Pointer(Pointer),
    Stop,
}

/// The player thread: send it work, await the pictures.
#[derive(Clone)]
pub struct Worker {
    tx: mpsc::Sender<Job>,
}

static WORKER: OnceLock<Worker> = OnceLock::new();

pub fn worker() -> Worker {
    WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            std::thread::Builder::new()
                .name("ruffle".into())
                // Ruffle recurses through display lists and ActionScript.
                .stack_size(32 << 20)
                .spawn(move || run(rx))
                .expect("the movie player thread starts");
            Worker { tx }
        })
        .clone()
}

impl Worker {
    fn ask<T>(&self, job: Job, rx: oneshot::Receiver<Result<T, String>>) -> impl std::future::Future<Output = Result<T, String>> + use<T> {
        let sent = self.tx.send(job);
        let busy = crate::backend::Busy::new();
        async move {
            let _busy = busy;
            sent.map_err(|_| "the movie player has stopped".to_string())?;
            rx.await.map_err(|_| "the movie player has stopped".to_string())?
        }
    }

    /// One picture of a movie: `frames` frames are run first (imports load meanwhile).
    pub fn still(&self, load: Load, frames: u32) -> impl std::future::Future<Output = Result<RgbaImage, String>> + use<> {
        let (reply, rx) = oneshot::channel();
        self.ask(Job::Still { load, frames, reply }, rx)
    }

    /// Make `load` the live player (the previous one is dropped).
    pub fn start(&self, load: Load) -> impl std::future::Future<Output = Result<Info, String>> + use<> {
        let (reply, rx) = oneshot::channel();
        self.ask(Job::Start { load, reply }, rx)
    }

    /// Run the live player `frames` frames (0: none), at `size` when given, and draw it.
    pub fn advance(&self, frames: u32, size: Option<(u32, u32)>) -> impl std::future::Future<Output = Result<RgbaImage, String>> + use<> {
        let (reply, rx) = oneshot::channel();
        self.ask(Job::Advance { frames, size, reply }, rx)
    }

    pub fn pointer(&self, p: Pointer) {
        let _ = self.tx.send(Job::Pointer(p));
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Job::Stop);
    }
}

/// A player with what keeps its imports loading.
struct Live {
    player: Arc<Mutex<Player>>,
    executor: NullExecutor,
    dir: PathBuf,
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Live {
    fn frames(&mut self, n: u32) {
        for _ in 0..n {
            self.executor.run();
            let mut p = self.player.lock().unwrap();
            p.preload(&mut ExecutionLimit::none());
            p.run_frame();
        }
        self.executor.run();
    }

    fn capture(&mut self) -> Result<RgbaImage, String> {
        let mut p = self.player.lock().unwrap();
        p.render();
        let renderer = <dyn std::any::Any>::downcast_mut::<WgpuRenderBackend<TextureTarget>>(p.renderer_mut()).ok_or("not the wgpu renderer")?;
        renderer.capture_frame().ok_or_else(|| "no picture".to_string())
    }

    fn resize(&mut self, w: u32, h: u32) {
        let mut p = self.player.lock().unwrap();
        let d = p.viewport_dimensions();
        if (d.width, d.height) != (w, h) {
            let movie_w = p.mutate_with_update_context(|ctx| ctx.root_swf.width().to_pixels());
            p.set_viewport_dimensions(ViewportDimensions { width: w, height: h, scale_factor: w as f64 / movie_w.max(1.0) });
        }
    }
}

fn descriptors() -> Result<Arc<Descriptors>, String> {
    static D: OnceLock<Result<Arc<Descriptors>, String>> = OnceLock::new();
    D.get_or_init(|| {
        let instance = create_wgpu_instance(wgpu::Backends::PRIMARY, wgpu::BackendOptions::default(), None);
        let (adapter, device, queue) = pollster::block_on(request_adapter_and_device(wgpu::Backends::PRIMARY, &instance, None, wgpu::PowerPreference::HighPerformance)).map_err(|e| format!("no GPU for movies: {e}"))?;
        Ok(Arc::new(Descriptors::new(instance, adapter, device, queue)))
    })
    .clone()
}

fn build(load: Load) -> Result<Live, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("tempest-studio-swf-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // The movie sits three folders down, so `../Lib/lib.swf` imports resolve inside `dir` too.
    let home = dir.join("a").join("b").join("c");
    for (url, bytes) in &load.libraries {
        let mut f = home.clone();
        for part in url.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => {
                    if f != dir {
                        f.pop();
                    }
                }
                p => f.push(p),
            }
        }
        std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&f, bytes).map_err(|e| e.to_string())?;
    }
    let movie_path = home.join("movie.swf");
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let url = format!("file://{}", movie_path.display());
    let movie = SwfMovie::from_data(&load.swf, url, None, None).map_err(|e| format!("Ruffle cannot read the movie: {e}"))?;
    let movie_w = movie.width().to_pixels().max(1.0);
    let d = descriptors()?;
    let (w, h) = (load.width.clamp(16, 4096), load.height.clamp(16, 4096));
    let target = TextureTarget::new(&d.device, (w, h)).map_err(|e| e.to_string())?;
    let executor = NullExecutor::new();
    let navigator = NullNavigatorBackend::with_base_path(&home, &executor).map_err(|e| e.to_string())?;
    let renderer = WgpuRenderBackend::new(d, target).map_err(|e| e.to_string())?;
    let player = PlayerBuilder::new().with_renderer(renderer).with_navigator(navigator).with_movie(movie).with_viewport_dimensions(w, h, w as f64 / movie_w).build();
    if load.transparent {
        player.lock().unwrap().set_window_mode("transparent");
    }
    Ok(Live { player, executor, dir })
}

fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(e) => Err(format!("the movie player failed: {}", e.downcast_ref::<String>().map(|s| s.as_str()).or_else(|| e.downcast_ref::<&str>().copied()).unwrap_or("panic"))),
    }
}

fn run(rx: mpsc::Receiver<Job>) {
    let mut live: Option<Live> = None;
    while let Ok(job) = rx.recv() {
        match job {
            Job::Still { load, frames, reply } => {
                let _ = reply.send(guarded(|| {
                    let mut l = build(load)?;
                    l.frames(frames);
                    l.capture()
                }));
            }
            Job::Start { load, reply } => {
                live = None;
                let r = guarded(|| {
                    let mut l = build(load)?;
                    l.frames(1);
                    let info = {
                        let mut p = l.player.lock().unwrap();
                        let (w, h) = p.mutate_with_update_context(|ctx| (ctx.root_swf.width().to_pixels(), ctx.root_swf.height().to_pixels()));
                        Info { frame_rate: p.frame_rate(), width: w, height: h }
                    };
                    live = Some(l);
                    Ok(info)
                });
                if r.is_err() {
                    live = None;
                }
                let _ = reply.send(r);
            }
            Job::Advance { frames, size, reply } => {
                let r = match &mut live {
                    Some(l) => guarded(|| {
                        if let Some((w, h)) = size {
                            l.resize(w.clamp(16, 4096), h.clamp(16, 4096));
                        }
                        l.frames(frames);
                        l.capture()
                    }),
                    None => Err("no movie is playing".into()),
                };
                if r.is_err() {
                    live = None;
                }
                let _ = reply.send(r);
            }
            Job::Pointer(e) => {
                if let Some(l) = &mut live {
                    let event = match e {
                        Pointer::Move(x, y) => PlayerEvent::MouseMove { x, y },
                        Pointer::Down(x, y) => PlayerEvent::MouseDown { x, y, button: MouseButton::Left, index: None },
                        Pointer::Up(x, y) => PlayerEvent::MouseUp { x, y, button: MouseButton::Left },
                        Pointer::Leave => PlayerEvent::MouseLeave,
                    };
                    let _ = guarded(|| {
                        l.player.lock().unwrap().handle_event(event);
                        Ok(())
                    });
                }
            }
            Job::Stop => live = None,
        }
    }
}

/// A Ruffle picture (RGBA, straight alpha) as a GPUI image (BGRA).
pub fn to_render_image(img: RgbaImage) -> Arc<gpui::RenderImage> {
    let mut img = img;
    for p in img.pixels_mut() {
        p.0.swap(0, 2);
    }
    Arc::new(gpui::RenderImage::new(vec![image::Frame::new(img)]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    const DIR: &str = "/Users/tom/Games/Tempest/8.1/ChaosGame/CookedPCConsole";

    /// The lobby's home screen through the backend and the player thread
    /// (needs the 8.1 game: `cargo test swf_player -- --ignored`).
    #[test]
    #[ignore]
    fn renders_the_home_screen() {
        let (b, _events) = crate::backend::Backend::start();
        let call = |m: &str, p: serde_json::Value| pollster::block_on(b.call(m, p)).unwrap();
        call("workspace.open", json!({ "dir": DIR }));
        let r = call("swf.preview", json!({ "path": format!("{DIR}/LobbyUI_SF.upk"), "index": 398, "target": { "kind": "root", "frame": 0 } }));
        let libraries = r.json["libraries"].as_array().unwrap().iter().map(|l| (l["url"].as_str().unwrap().to_string(), r.bin(&l["swf"]).unwrap().to_vec())).collect();
        let load = super::Load { swf: r.bin(&r.json["swf"]).unwrap().to_vec(), libraries, width: 960, height: 540, transparent: false };
        let t = std::time::Instant::now();
        let img = pollster::block_on(super::worker().still(load, 12)).unwrap();
        eprintln!("still in {} ms", t.elapsed().as_millis());
        assert_eq!((img.width(), img.height()), (960, 540));
        // The champion card's art: some strongly coloured pixels, not just the grey background.
        let coloured = img.pixels().filter(|p| p.0[0].abs_diff(p.0[1]) > 60 || p.0[1].abs_diff(p.0[2]) > 60).count();
        assert!(coloured > 2000, "{coloured} coloured pixels");
    }
}
