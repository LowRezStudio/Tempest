//! A test driver for checking the UI without a person: with
//! `TEMPEST_DRIVE=1` the app reads steps from stdin, one per line, runs each
//! on the window, waits until no work is in flight and the frame is drawn,
//! then prints `ok` (or `err <why>`). `scripts/drive.py` sends the steps and
//! takes the screenshots.
//!
//! Steps: `key <keystroke>…` (`cmd-k`, `down`, `enter`), `type <text>`,
//! `click <x> <y> [right|middle] [double] [cmd|shift|alt|ctrl]…` and
//! `move`/`drag <x1> <y1> <x2> <y2>`/`scroll <x> <y> <dy>` in window
//! points, `click @<probe>` at the centre of a probed element, `wait <ms>`,
//! `size <w> <h>`, and anything else goes to `Studio::drive` (`package
//! <name>`, `select <index|name>`, `run <command id>`, …).

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui::{canvas, point, prelude::*, px, size, AnyWindowHandle, App, AsyncApp, Bounds, Entity, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollEvent, Window};

use crate::studio::Studio;

thread_local! {
    static PROBES: RefCell<HashMap<String, Bounds<Pixels>>> = RefCell::new(HashMap::new());
}

/// An invisible child that records its parent's bounds under `name`, so a
/// step can click it (`click @name`). Only laid out when driving.
pub fn probe(name: impl Into<String>) -> impl IntoElement {
    let name = name.into();
    canvas(
        move |bounds, _, _| {
            PROBES.with(|p| p.borrow_mut().insert(name.clone(), bounds));
        },
        |_, _, _, _| {},
    )
    // Pinned to the corner: an absolute child without insets sits where the flow would put it.
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

pub fn enabled() -> bool {
    std::env::var_os("TEMPEST_DRIVE").is_some()
}

/// Start reading steps (when driving).
pub fn start(window: AnyWindowHandle, studio: Entity<Studio>, cx: &mut App) {
    if !enabled() {
        return;
    }
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<String>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        while stdin.read_line(&mut line).is_ok_and(|n| n > 0) {
            let _ = tx.unbounded_send(line.trim().to_string());
            line.clear();
        }
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        use futures::StreamExt;
        while let Some(step) = rx.next().await {
            if step.is_empty() {
                continue;
            }
            let r = run(&step, window, &studio, cx).await;
            settle(cx).await;
            let mut out = std::io::stdout();
            let _ = match r {
                Ok(()) => writeln!(out, "ok"),
                Err(e) => writeln!(out, "err {e}"),
            };
            let _ = out.flush();
        }
    })
    .detach();
}

/// Wait until nothing is in flight for a few polls, then for a frame.
async fn settle(cx: &mut AsyncApp) {
    let mut quiet = 0;
    for _ in 0..600 {
        cx.background_executor().timer(Duration::from_millis(25)).await;
        if crate::backend::WORK.load(Ordering::SeqCst) == 0 {
            quiet += 1;
            if quiet >= 4 {
                break;
            }
        } else {
            quiet = 0;
        }
    }
    cx.background_executor().timer(Duration::from_millis(80)).await;
}

fn modifiers(words: &[&str]) -> Modifiers {
    let mut m = Modifiers::default();
    for w in words {
        match *w {
            "cmd" => m.platform = true,
            "shift" => m.shift = true,
            "alt" => m.alt = true,
            "ctrl" => m.control = true,
            _ => {}
        }
    }
    m
}

/// A point of a step: `@probe` (its centre), `@probe:x,y` (that far from its
/// top-left corner), else two numbers.
fn at(words: &[&str]) -> Result<Point<Pixels>, String> {
    if let Some((name, off)) = words.first().and_then(|w| w.strip_prefix('@')).and_then(|w| w.rsplit_once(':')).filter(|(_, o)| o.contains(',')) {
        let b = PROBES.with(|p| p.borrow().get(name).copied()).ok_or_else(|| format!("no probe {name} on screen"))?;
        let (x, y) = off.split_once(',').ok_or("@probe:x,y")?;
        let (x, y) = (x.parse::<f32>().map_err(|e| e.to_string())?, y.parse::<f32>().map_err(|e| e.to_string())?);
        return Ok(point(b.origin.x + px(x), b.origin.y + px(y)));
    }
    if let Some(name) = words.first().and_then(|w| w.strip_prefix('@')) {
        let b = PROBES.with(|p| p.borrow().get(name).copied()).ok_or_else(|| {
            // Name the probes of the same family, so a step can pick one on screen.
            let family = name.rsplit_once(':').map_or(name, |(f, _)| f);
            let mut near: Vec<String> = PROBES.with(|p| p.borrow().keys().filter(|k| k.starts_with(family)).cloned().collect());
            near.sort();
            near.truncate(20);
            format!("no probe {name} on screen (on screen: {})", near.join(" "))
        })?;
        // The content runs under the transparent title bar: probes and input
        // events share the window's coordinates.
        return Ok(b.center());
    }
    let n = |k: usize| words.get(k).and_then(|w| w.parse::<f32>().ok()).ok_or_else(|| "give x y".to_string());
    Ok(point(px(n(0)?), px(n(1)?)))
}

async fn run(step: &str, window: AnyWindowHandle, studio: &Entity<Studio>, cx: &mut AsyncApp) -> Result<(), String> {
    let (verb, rest) = step.split_once(' ').unwrap_or((step, ""));
    let words: Vec<&str> = rest.split_whitespace().collect();
    let on_window = |cx: &mut AsyncApp, f: &mut dyn FnMut(&mut Window, &mut App)| cx.update_window(window, |_, window, cx| f(window, cx)).map_err(|e| e.to_string());
    match verb {
        "wait" => {
            let ms = rest.trim().parse::<u64>().map_err(|_| "wait <ms>")?;
            cx.background_executor().timer(Duration::from_millis(ms)).await;
            Ok(())
        }
        "key" => {
            for k in words.iter().map(|w| -> &'static str { Box::leak(w.to_string().into_boxed_str()) }).collect::<Vec<_>>() {
                let ks = Keystroke::parse(k).map_err(|e| e.to_string())?;
                on_window(cx, &mut |window, cx| {
                    window.dispatch_keystroke(ks.clone(), cx);
                })?;
                cx.background_executor().timer(Duration::from_millis(30)).await;
            }
            Ok(())
        }
        "type" => {
            for ch in rest.chars() {
                let s = ch.to_string();
                let ks = Keystroke { modifiers: Modifiers::default(), key: if ch == ' ' { "space".into() } else { s.clone() }, key_char: Some(s) };
                on_window(cx, &mut |window, cx| {
                    window.dispatch_keystroke(ks.clone(), cx);
                })?;
            }
            Ok(())
        }
        "size" => {
            let n = |k: usize| words.get(k).and_then(|w| w.parse::<f32>().ok()).ok_or("size <w> <h>");
            let (w, h) = (n(0)?, n(1)?);
            on_window(cx, &mut |window, _| window.resize(size(px(w), px(h))))
        }
        // Where a probe is (printed before `ok`), for steps that aim inside it.
        "probe" => {
            let name = words.first().ok_or("probe <name>")?;
            let b = PROBES.with(|p| p.borrow().get(*name).copied()).ok_or_else(|| format!("no probe {name} on screen"))?;
            println!("probe {name} {} {} {} {}", f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height));
            Ok(())
        }
        "move" => {
            let p = at(&words)?;
            on_window(cx, &mut |window, cx| {
                window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, pressed_button: None, modifiers: Modifiers::default() }), cx);
            })
        }
        "click" => {
            let p = at(&words)?;
            let button = if words.contains(&"right") {
                MouseButton::Right
            } else if words.contains(&"middle") {
                MouseButton::Middle
            } else {
                MouseButton::Left
            };
            let clicks = if words.contains(&"double") { 2 } else { 1 };
            let m = modifiers(&words);
            for c in 1..=clicks {
                on_window(cx, &mut |window, cx| {
                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, pressed_button: None, modifiers: m }), cx);
                    window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button, position: p, modifiers: m, click_count: c, first_mouse: false }), cx);
                    window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button, position: p, modifiers: m, click_count: c }), cx);
                })?;
            }
            Ok(())
        }
        "drag" => {
            // Two points: `@probe:x,y` each, or four numbers.
            let (a, b) = if words.first().is_some_and(|w| w.starts_with('@')) {
                (at(&words[0..1])?, at(words.get(1..2).ok_or("drag @probe:x,y @probe:x,y")?)?)
            } else {
                let n = |k: usize| words.get(k).and_then(|w| w.parse::<f32>().ok()).ok_or("drag <x1> <y1> <x2> <y2>");
                (point(px(n(0)?), px(n(1)?)), point(px(n(2)?), px(n(3)?)))
            };
            let m = modifiers(&words);
            on_window(cx, &mut |window, cx| {
                window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: a, pressed_button: None, modifiers: m }), cx);
                window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: a, modifiers: m, click_count: 1, first_mouse: false }), cx);
            })?;
            for k in 1..=10 {
                let p = a + (b - a) * (k as f32 / 10.0);
                on_window(cx, &mut |window, cx| {
                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, pressed_button: Some(MouseButton::Left), modifiers: m }), cx);
                })?;
                cx.background_executor().timer(Duration::from_millis(16)).await;
            }
            on_window(cx, &mut |window, cx| {
                window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: b, modifiers: m, click_count: 1 }), cx);
            })
        }
        "scroll" => {
            let p = at(&words)?;
            let k = if words.first().is_some_and(|w| w.starts_with('@')) { 1 } else { 2 };
            let dy = words.get(k).and_then(|w| w.parse::<f32>().ok()).ok_or("scroll <x> <y> <dy> [dx]")?;
            let dx = words.get(k + 1).and_then(|w| w.parse::<f32>().ok()).unwrap_or(0.0);
            on_window(cx, &mut |window, cx| {
                window.dispatch_event(PlatformInput::Scroll(ScrollEvent { position: p, delta: ScrollDelta::Pixels(point(px(dx), px(dy))), modifiers: Modifiers::default(), touch_phase: Default::default(), source: Default::default() }), cx);
            })
        }
        // The views are their own entities: updated outside the Studio's update.
        "map" => {
            let rest = rest.to_string();
            let studio = studio.clone();
            cx.update_window(window, |_, window, cx| {
                let m = studio.read(cx).map_entity().ok_or("the map view is not open (view map)")?;
                m.update(cx, |m, cx| m.drive(&rest, window, cx))
            })
            .map_err(|e| e.to_string())?
        }
        "assembly" | "cooker" => {
            let rest = rest.to_string();
            let studio = studio.clone();
            cx.update_window(window, |_, window, cx| {
                let s = studio.read(cx);
                if verb == "assembly" {
                    let a = s.assembly_entity().ok_or("the assembly view is not open")?;
                    a.update(cx, |a, cx| a.drive(&rest, window, cx))
                } else {
                    let c = s.cooker_entity().ok_or("the mod view is not open")?;
                    c.update(cx, |c, cx| c.drive(&rest, window, cx))
                }
            })
            .map_err(|e| e.to_string())?
        }
        _ => {
            let (verb, rest) = (verb.to_string(), rest.to_string());
            let studio = studio.clone();
            cx.update_window(window, |_, window, cx| studio.update(cx, |s, cx| s.drive(&verb, &rest, window, cx))).map_err(|e| e.to_string())?
        }
    }
}
