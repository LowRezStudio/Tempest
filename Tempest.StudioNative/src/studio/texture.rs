//! The texture preview: the picture at any mip that is stored, fitted or at
//! 1:1 / 2× (scrolls when larger than the pane), on a dark, grey or light
//! background, with its size, format and where each mip lives.

use std::sync::Arc;

use gpui::{div, img, prelude::*, px, AnyElement, Context, Image, ImageFormat, ObjectFit};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Sizable};
use serde_json::json;

use super::Studio;

pub struct TexView {
    pub export: usize,
    pub format: String,
    /// (width, height, where it is: inline / a .tfc / missing)
    pub mips: Vec<(u64, u64, String)>,
    pub mip: usize,
    /// None: fit the pane; else the scale.
    pub zoom: Option<f32>,
    /// 0 dark, 1 grey, 2 light.
    pub bg: u8,
}

impl Studio {
    /// Mips and format of texture `i` (the picture itself comes with the selection).
    pub(super) fn load_texture_info(&mut self, path: String, i: usize, cx: &mut Context<Self>) {
        let keep = self.tex.as_ref().map(|t| (t.zoom, t.bg)).unwrap_or((None, 0));
        self.tex = Some(TexView { export: i, format: String::new(), mips: Vec::new(), mip: 0, zoom: keep.0, bg: keep.1 });
        let call = self.backend.call("texture.info", json!({ "path": path, "index": i }));
        cx.spawn(async move |this, cx| {
            let Ok(r) = call.await else { return };
            let _ = this.update(cx, |this, cx| {
                let Some(t) = this.tex.as_mut().filter(|t| t.export == i) else { return };
                t.format = r.json["format"].as_str().unwrap_or("").trim_start_matches("PF_").to_string();
                t.mips = r.json["mips"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|m| {
                        let at = match m["location"]["kind"].as_str() {
                            Some("cache") => m["location"]["file"].as_str().map(super::file_name).unwrap_or_else(|| "texture cache".into()),
                            Some("inline") => "in the package".into(),
                            Some(k) => k.to_string(),
                            None => String::new(),
                        };
                        (m["width"].as_u64().unwrap_or(0), m["height"].as_u64().unwrap_or(0), at)
                    })
                    .collect();
                cx.notify();
            });
        })
        .detach();
    }

    fn show_mip(&mut self, mip: usize, cx: &mut Context<Self>) {
        let (Some(t), Some(path)) = (self.tex.as_mut(), self.open.as_ref().map(|o| o.path.clone())) else { return };
        t.mip = mip;
        let i = t.export;
        let call = self.backend.call("texture.preview", json!({ "path": path, "index": i, "mip": mip }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.selected_export != Some(i) {
                    return;
                }
                match r {
                    Ok(r) => this.preview = r.bin(&r.json["png"]).map(|png| Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec()))),
                    Err(e) => this.set_status(format!("Mip {mip}: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_texture(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let image = self.preview.clone()?;
        let t = self.tex.as_ref();
        let (zoom, bg, mip) = t.map(|t| (t.zoom, t.bg, t.mip)).unwrap_or((None, 0, 0));
        let (w, h, at) = t.and_then(|t| t.mips.get(t.mip).cloned()).unwrap_or((0, 0, String::new()));
        let format = t.map(|t| t.format.clone()).unwrap_or_default();
        let back = match bg {
            1 => gpui::hsla(0.0, 0.0, 0.5, 1.0),
            2 => gpui::hsla(0.0, 0.0, 0.92, 1.0),
            _ => theme.muted,
        };
        let pic: AnyElement = match zoom {
            None => div().size_full().flex().items_center().justify_center().p_2().child(img(image).max_w_full().max_h_full().object_fit(ObjectFit::Contain)).into_any_element(),
            Some(z) => div()
                .id("tex-scroll")
                .size_full()
                .overflow_scroll()
                .child(div().p_2().child(img(image).w(px(w.max(1) as f32 * z)).h(px(h.max(1) as f32 * z))))
                .into_any_element(),
        };
        let zb = |id: &'static str, label: &'static str, z: Option<f32>, cx: &mut Context<Self>| {
            let b = Button::new(id).xsmall().label(label).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(t) = &mut this.tex {
                    t.zoom = z;
                }
                cx.notify();
            }));
            if zoom == z { b.primary() } else { b.ghost() }
        };
        let me = cx.entity().downgrade();
        let mips = t.map(|t| t.mips.clone()).unwrap_or_default();
        let mip_menu = Button::new("tex-mip").xsmall().ghost().label(format!("Mip {mip}")).tooltip("Show a smaller mip: the game draws them at a distance").dropdown_menu(move |menu, _, _| {
            let mut menu = menu;
            for (k, (mw, mh, at)) in mips.iter().enumerate() {
                let me = me.clone();
                menu = menu.item(PopupMenuItem::new(format!("{k} · {mw}×{mh} · {at}")).disabled(at == "missing" || at == "unused").on_click(move |_, _, cx| {
                    let _ = me.update(cx, |this, cx| this.show_mip(k, cx));
                }));
            }
            menu
        });
        let bgb = |id: &'static str, label: &'static str, v: u8, cx: &mut Context<Self>| {
            let b = Button::new(id).xsmall().label(label).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(t) = &mut this.tex {
                    t.bg = v;
                }
                cx.notify();
            }));
            if bg == v { b.primary() } else { b.ghost() }
        };
        let bar = h_flex()
            .gap_1()
            .child(zb("tex-fit", "Fit", None, cx))
            .child(zb("tex-1", "1:1", Some(1.0), cx))
            .child(zb("tex-2", "2×", Some(2.0), cx))
            .child(div().w(px(8.0)))
            .child(mip_menu)
            .child(div().w(px(8.0)))
            .child(bgb("tex-bg-dark", "Dark", 0, cx))
            .child(bgb("tex-bg-grey", "Grey", 1, cx))
            .child(bgb("tex-bg-light", "Light", 2, cx));
        let info = if w > 0 { format!("{w}×{h} · {format} · mip {mip} {at}") } else { String::new() };
        Some(
            v_flex()
                .gap_1()
                .child(bar)
                .child(div().w_full().h(px(340.0)).flex_none().overflow_hidden().rounded_md().bg(back).child(pic))
                .child(div().text_xs().text_color(theme.muted_foreground).child(info))
                .into_any_element(),
        )
    }
}
