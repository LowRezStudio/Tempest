//! Static light baking for the open map (`light.bake`, toolbar Light): the
//! scope (meshes added or moved this session, the selection, or whole
//! levels), the ray counts and bounce albedo, a comparison with the light
//! maps the meshes had, and the selected mesh's light map as stored
//! (`light.info`: lights, textures, scales, a decoded irradiance picture
//! with an exposure slider). Every level written is one undo step.

use std::collections::HashSet;
use std::sync::Arc;

use gpui::{div, img, prelude::*, px, AnyElement, Context, ObjectFit, RenderImage, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Selectable, Sizable};
use serde_json::{json, Value as J};

use super::MapEditor;
use crate::studio::Tone;

/// A light map read: the level, the component and the exposure (×100).
type InfoKey = (String, usize, u32);

pub struct Bake {
    pub open: bool,
    pub scope: &'static str,
    pub levels: HashSet<String>,
    pub shadow_samples: u32,
    pub sky_samples: u32,
    pub indirect_rays: u32,
    pub albedo: f64,
    pub compare: bool,
    pub busy: bool,
    pub last: Option<J>,
    /// The selected mesh's light map: for which component, and the picture.
    info: Option<(InfoKey, Result<J, String>)>,
    picture: Option<Arc<RenderImage>>,
    exposure: Option<gpui::Entity<SliderState>>,
    exposure_value: f32,
    _sub: Option<gpui::Subscription>,
}

impl Default for Bake {
    fn default() -> Bake {
        Bake { open: false, scope: "edited", levels: HashSet::new(), shadow_samples: 8, sky_samples: 64, indirect_rays: 0, albedo: 0.08, compare: true, busy: false, last: None, info: None, picture: None, exposure: None, exposure_value: 1.0, _sub: None }
    }
}

impl MapEditor {
    pub(super) fn open_bake(&mut self, cx: &mut Context<Self>) {
        self.bake.open = true;
        cx.notify();
    }

    /// Bake `scope` and reload the levels written.
    pub(crate) fn bake_now(&mut self, scope: &str, targets: Option<J>, levels: Option<Vec<String>>, extra: J, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else { return };
        if self.bake.busy {
            return;
        }
        let targets = match (scope, targets) {
            (_, Some(t)) => Some(t),
            ("selected", None) => match self.selected.clone() {
                Some((l, e)) => Some(json!([{ "path": l, "index": e }])),
                None => {
                    self.toast(Tone::Info, "Select a mesh or an actor first", cx);
                    return;
                }
            },
            _ => None,
        };
        let levels = levels.or_else(|| (scope == "level").then(|| self.bake.levels.iter().cloned().collect()));
        let mut params = json!({ "path": path, "scope": scope, "targets": targets, "levels": levels, "shadowSamples": self.bake.shadow_samples, "skySamples": self.bake.sky_samples, "indirectRays": self.bake.indirect_rays, "albedo": self.bake.albedo, "compare": self.bake.compare });
        if let Some(o) = extra.as_object() {
            for (k, v) in o {
                if !v.is_null() {
                    params[k] = v.clone();
                }
            }
        }
        self.bake.busy = true;
        let call = self.call("light.bake", params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.bake.busy = false;
                this.progress = None;
                match r {
                    Ok(r) => {
                        for w in r.json["written"].as_array().into_iter().flatten() {
                            if let Some(p) = w["path"].as_str() {
                                this.note_edit(p, cx);
                                this.reload_level(p.to_string(), cx);
                            }
                        }
                        let j = &r.json;
                        let n = j["baked"].as_array().map(|a| a.len()).unwrap_or(0);
                        let written = j["written"].as_array().map(|a| a.len()).unwrap_or(0);
                        let skipped = j["skipped"].as_array().map(|a| a.len()).unwrap_or(0);
                        let removed: usize = j["written"].as_array().into_iter().flatten().map(|w| w["removed"].as_array().map(|a| a.len()).unwrap_or(0)).sum();
                        let text = if n > 0 {
                            format!(
                                "Lighting baked: {n} mesh{} in {written} level{}, {:.1} s{}{}",
                                if n == 1 { "" } else { "es" },
                                if written == 1 { "" } else { "s" },
                                j["seconds"].as_f64().unwrap_or(0.0),
                                if skipped > 0 { format!("; {skipped} skipped") } else { String::new() },
                                if removed > 0 { format!("; {removed} old light map texture{} removed", if removed == 1 { "" } else { "s" }) } else { String::new() }
                            )
                        } else if let Some(s) = j["skipped"].get(0) {
                            format!("Nothing baked: {}", s["reason"].as_str().unwrap_or(""))
                        } else {
                            "Nothing to bake in this scope".into()
                        };
                        this.toast(if n > 0 { Tone::Success } else { Tone::Info }, text, cx);
                        this.bake.last = Some(r.json);
                        this.bake.info = None;
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not bake the lighting: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The selected actor's first static mesh component.
    fn bake_component(&self) -> Option<(String, usize)> {
        let (l, _) = self.selected.clone()?;
        let a = self.actor()?;
        let p = a.parts.iter().find(|p| p["kind"] == "mesh")?;
        Some((l, p["export"].as_u64()? as usize))
    }

    fn load_light_info(&mut self, cx: &mut Context<Self>) {
        let Some((l, e)) = self.bake_component() else {
            self.bake.info = None;
            return;
        };
        let exposure = self.bake.exposure_value;
        let key = (l.clone(), e, (exposure * 100.0) as u32);
        if self.bake.info.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        self.bake.info = Some((key.clone(), Ok(J::Null)));
        let call = self.call("light.info", json!({ "path": l, "index": e, "preview": true, "exposure": exposure }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.bake.info.as_ref().map(|i| &i.0) != Some(&key) {
                    return;
                }
                match r {
                    Ok(r) => {
                        this.bake.picture = r.bin(&r.json["png"]).and_then(|png| image::load_from_memory(png).ok()).map(|i| crate::swf_player::to_render_image(i.to_rgba8()));
                        this.bake.info = Some((key, Ok(r.json)));
                    }
                    Err(e) => this.bake.info = Some((key, Err(e))),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn commit_bake_field(&mut self, key: &str, value: String, cx: &mut Context<Self>) {
        let Ok(n) = value.trim().parse::<f64>() else { return };
        match key {
            "shadow" => self.bake.shadow_samples = (n as u32).clamp(1, 64),
            "sky" => self.bake.sky_samples = (n as u32).clamp(1, 512),
            "indirect" => self.bake.indirect_rays = (n as u32).min(256),
            "albedo" => self.bake.albedo = n.clamp(0.0, 1.0),
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn render_bake(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.bake.open {
            return None;
        }
        let theme = cx.theme().clone();
        self.load_light_info(cx);
        if self.bake.exposure.is_none() {
            let s = cx.new(|_| SliderState::new().min(0.25).max(8.0).step(0.25).default_value(1.0));
            self.bake._sub = Some(cx.subscribe(&s, |this, _, e: &SliderEvent, cx| {
                // Read again once the slider is let go (each read renders a picture).
                if let SliderEvent::Release(v) = e {
                    this.bake.exposure_value = v.end();
                    cx.notify();
                }
            }));
            self.bake.exposure = Some(s);
        }
        let _ = window;
        let mut body = v_flex().gap_3().text_xs();
        body = body.child(div().text_color(theme.muted_foreground).child("Direct light from the map's static lights (point, spot, directional, sky) with ray-traced shadows against every level, stored like Lightmass stores it (new light map textures in the level, the meshes pointed at them). Indirect light is an optional one-bounce approximation, so bakes come out darker than Lightmass in shaded areas."));
        let mut scopes = h_flex().gap_1();
        for (id, label, detail) in [("edited", "Edited", "meshes added or moved this session"), ("selected", "Selected", "the selected mesh or actor"), ("level", "Levels", "every mesh of the levels ticked (slow)")] {
            scopes = scopes.child(Button::new(id).small().label(label).tooltip(detail).selected(self.bake.scope == id).on_click(cx.listener(move |this, _, _, cx| {
                this.bake.scope = id;
                cx.notify();
            })));
        }
        body = body.child(scopes);
        match self.bake.scope {
            "level" => {
                let mut grid = div().grid().grid_cols(2).gap_x_3();
                for (k, l) in self.levels.iter().enumerate() {
                    let p = l.path.clone();
                    grid = grid.child(Checkbox::new(("bake-lvl", k)).label(l.name.clone()).checked(self.bake.levels.contains(&l.path)).on_click(cx.listener(move |this, v: &bool, _, cx| {
                        if *v {
                            this.bake.levels.insert(p.clone());
                        } else {
                            this.bake.levels.remove(&p);
                        }
                        cx.notify();
                    })));
                }
                body = body.child(grid).child(div().text_color(theme.warning).child("A whole level takes from seconds to minutes (Abyss' walls: 187 meshes in about 15 s without indirect light; indirect rays multiply it)."));
            }
            "selected" => body = body.child(div().text_color(theme.muted_foreground).child(self.actor().map(|a| a.name.clone()).unwrap_or_else(|| "Nothing selected.".into()))),
            _ => {}
        }
        let row = |label: &str, field: AnyElement| h_flex().justify_between().gap_2().child(label.to_string()).child(field);
        body = body.child(
            div()
                .grid()
                .grid_cols(2)
                .gap_2()
                .child(row("Shadow rays per light", self.field("bake:shadow", self.bake.shadow_samples.to_string(), 70.0, cx)))
                .child(row("Sky rays", self.field("bake:sky", self.bake.sky_samples.to_string(), 70.0, cx)))
                .child(row("Indirect rays (0: none)", self.field("bake:indirect", self.bake.indirect_rays.to_string(), 70.0, cx)))
                .child(row("Bounce albedo", self.field("bake:albedo", format!("{}", self.bake.albedo), 70.0, cx))),
        );
        if self.bake.indirect_rays == 0 {
            body = body.child(div().text_color(theme.muted_foreground).child("Without indirect rays, areas out of direct light come out at about 0.6 of the game's own lighting; 32 to 64 rays bring them close."));
        }
        body = body.child(Checkbox::new("bake-compare").label("Compare with the light maps the meshes had").checked(self.bake.compare).on_click(cx.listener(|this, v: &bool, _, cx| {
            this.bake.compare = *v;
            cx.notify();
        })));
        if let Some(r) = &self.bake.last {
            let lod0: Vec<&J> = r["comparisons"].as_array().into_iter().flatten().filter(|c| c["lod"] == 0).collect();
            let mut res = v_flex().p_2().rounded_md().bg(theme.secondary).child(format!(
                "{} baked, {} skipped, {} lights, {} shadow triangles, {:.1} s",
                r["baked"].as_array().map(|a| a.len()).unwrap_or(0),
                r["skipped"].as_array().map(|a| a.len()).unwrap_or(0),
                r["lights"],
                r["triangles"],
                r["seconds"].as_f64().unwrap_or(0.0)
            ));
            if !lod0.is_empty() {
                let m: f64 = lod0.iter().map(|c| c["stats"]["meanBaked"].as_f64().unwrap_or(0.0)).sum::<f64>() / lod0.len() as f64;
                let s: f64 = lod0.iter().map(|c| c["stats"]["meanShipped"].as_f64().unwrap_or(0.0)).sum::<f64>() / lod0.len() as f64;
                res = res.child(format!("Mean irradiance {m:.3} baked vs {s:.3} before ({} meshes)", lod0.len()));
            }
            for s in r["skipped"].as_array().into_iter().flatten().take(5) {
                res = res.child(div().text_color(theme.muted_foreground).child(format!("{}: {}", s["name"].as_str().unwrap_or(""), s["reason"].as_str().unwrap_or(""))));
            }
            body = body.child(res);
        }
        if self.bake_component().is_some() {
            let fmt = |v: &J| v.as_array().map(|a| a.iter().map(|x| format!("{:.3}", x.as_f64().unwrap_or(0.0))).collect::<Vec<_>>().join(" ")).unwrap_or_default();
            let mut info = v_flex().gap_1().pt_2().border_t_1().border_color(theme.border).child(h_flex().gap_2().child(div().font_weight(gpui::FontWeight::MEDIUM).child("Selected mesh's light map")).child(div().flex_1()).child(div().text_color(theme.muted_foreground).child(format!("Exposure {:.2}", self.bake.exposure_value))).children(self.bake.exposure.as_ref().map(|s| div().w(px(110.0)).child(Slider::new(s)))));
            match self.bake.info.as_ref().map(|i| &i.1) {
                Some(Err(e)) => info = info.child(div().text_color(theme.danger).child(e.clone())),
                Some(Ok(j)) if !j.is_null() => {
                    let lod = &j["lods"][0];
                    let mut text = v_flex().min_w_0().text_color(theme.muted_foreground);
                    if lod.is_null() || lod["kind"] == "none" {
                        text = text.child("No light map.");
                    } else {
                        text = text.child(format!("{}, {} lights", lod["kind"].as_str().unwrap_or(""), lod["lights"].as_array().map(|a| a.len()).unwrap_or(0)));
                        for t in lod["textures"].as_array().into_iter().flatten() {
                            text = text.child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family("Menlo").child(format!("{} × {}", t["texture"].as_str().unwrap_or("—"), fmt(&t["scale"]))));
                        }
                        text = text.child(format!("scale {}, bias {}", fmt(&lod["coordinateScale"]), fmt(&lod["coordinateBias"])));
                        if !j["decoded"].is_null() {
                            text = text.child(format!("{}×{} texels, mean irradiance {}", j["decoded"]["width"], j["decoded"]["height"], fmt(&j["decoded"]["meanIrradiance"])));
                        } else if let Some(e) = j["decodeError"].as_str() {
                            text = text.child(e.to_string());
                        }
                    }
                    info = info.child(h_flex().items_start().gap_3().children(self.bake.picture.clone().map(|p| img(p).size(px(128.0)).object_fit(ObjectFit::Contain).rounded_md().border_1().border_color(theme.border))).child(text));
                }
                _ => info = info.child(div().text_color(theme.muted_foreground).child("Reading…")),
            }
            body = body.child(info);
        }
        let progress = self.progress.as_ref().filter(|p| p.0 == "light.progress").map(|p| format!(" {}/{}", p.1, p.2)).unwrap_or_else(|| "…".into());
        body = body.child(
            h_flex()
                .gap_2()
                .justify_end()
                .when(self.bake.busy, |d| d.child(div().mr_auto().text_color(theme.muted_foreground).child(format!("Baking{progress}"))))
                .child(Button::new("bake-close").small().label("Close").disabled(self.bake.busy).on_click(cx.listener(|this, _, _, cx| {
                    this.bake.open = false;
                    cx.notify();
                })))
                .child(Button::new("bake-run").small().primary().label(if self.bake.busy { "Baking…" } else { "Bake" }).disabled(self.bake.busy).on_click(cx.listener(|this, _, _, cx| {
                    let scope = this.bake.scope;
                    this.bake_now(scope, None, None, json!({}), cx);
                }))),
        );
        Some(super::palette::modal("Bake lighting", 560.0, body.into_any_element(), cx))
    }
}
