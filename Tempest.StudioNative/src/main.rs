// No console window behind a release build on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod audio;
mod backend;
mod driver;
mod mcp;
mod map_render;
mod mesh_view;
mod model;
mod particle_view;
mod particles;
mod screenshot;
mod shell;
mod studio;
mod swf_player;
mod update;

use gpui::App;

fn main() {
    // What a finished or abandoned update left, and the file associations of a bundled build.
    std::thread::spawn(|| {
        update::clean_leftovers();
        shell::register();
    });
    let app = gpui_platform::application().with_assets(assets::Assets);
    // Files opened from the OS arrive here; the app routes them once it runs.
    let (tx, rx) = futures::channel::mpsc::unbounded::<Vec<String>>();
    app.on_open_urls(move |urls| {
        let _ = tx.unbounded_send(urls);
    });
    app.run(|cx: &mut App| {
        gpui_component::init(cx);
        studio::init(cx);
        let studio = studio::open_window(Some(String::new()), cx);
        if let Some(studio) = studio {
            if let Some(w) = cx.windows().first().copied() {
                driver::start(w, studio, cx);
            }
        }
        cx.spawn(async move |cx| {
            use futures::StreamExt;
            let mut rx = rx;
            while let Some(urls) = rx.next().await {
                cx.update(|cx| studio::open_urls(urls, cx));
            }
        })
        .detach();
        cx.activate(true);
    });
}
