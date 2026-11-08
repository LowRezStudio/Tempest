//! What the properties pane edits and exports of one character, by kind
//! (`swf.character` with the pending edits applied): a shape's colours and
//! line widths (and Replace with SVG), a static text's records, every
//! property of an edit-text field, a font's characters (add more from a
//! TrueType/OpenType file, export as TrueType), a sound or binary data as a
//! file, a sprite's frames as PNGs. Changes are pending edits like the stage's.

use gpui::{div, prelude::*, px, AnyElement, Context, Hsla, PathPromptOptions, Rgba, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_component::input::{InputEvent, Textarea, TextareaState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{parse_hex, MovieEditor};

const FLAGS: &[(&str, &str)] = &[("multiline", "multiline"), ("wordWrap", "word wrap"), ("html", "HTML"), ("readOnly", "read-only"), ("noSelect", "not selectable"), ("password", "password"), ("autoSize", "auto size"), ("border", "border"), ("useOutlines", "embedded glyphs"), ("wasStatic", "was static")];

fn presets() -> Vec<(&'static str, String)> {
    vec![
        ("Latin-1", " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~¡¢£¤¥¦§¨©ª«¬®¯°±²³´µ¶·¸¹º»¼½¾¿ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖ×ØÙÚÛÜÝÞßàáâãäåæçèéêëìíîïðñòóôõö÷øùúûüýþÿ".to_string()),
        ("Latin Extended-A", (0x100u32..0x180).filter_map(char::from_u32).collect()),
        ("Cyrillic", (0x400u32..0x460).filter_map(char::from_u32).collect()),
        ("Greek", (0x391u32..0x3D0).filter_map(char::from_u32).collect()),
    ]
}

/// `#rrggbbaa` → its alpha in percent.
fn alpha_of(c: &str) -> f64 {
    (u8::from_str_radix(c.get(7..9).unwrap_or("ff"), 16).unwrap_or(255) as f64 / 255.0 * 100.0).round()
}

fn hsla_of(c: &str) -> Hsla {
    let [r, g, b] = parse_hex(c).unwrap_or([0, 0, 0]);
    Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }.into()
}

impl MovieEditor {
    /// A multi-line text that commits when it loses focus (`commit_field` with `key`).
    pub(super) fn textarea(&mut self, key: String, text: String, edited: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let fresh = !matches!(self.texts.get(&key), Some((t, ..)) if *t == text);
        if fresh {
            let state = cx.new(|cx| {
                let mut s = TextareaState::new(window, cx).rows(3);
                s.set_value(text.clone(), window, cx);
                s
            });
            let k = key.clone();
            let sub = cx.subscribe(&state, move |this, state, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Blur) {
                    let v = state.read(cx).value().to_string();
                    let unchanged = this.texts.get(&k).is_some_and(|(t, ..)| *t == v);
                    if !unchanged {
                        this.commit_field(&k, v, cx);
                    }
                }
            });
            self.texts.insert(key.clone(), (text, state, sub));
        }
        let state = self.texts[&key].1.clone();
        div().rounded_md().when(edited, |d| d.border_1().border_color(theme.warning)).child(Textarea::new(&state).h(px(64.0))).into_any_element()
    }

    /// A colour swatch with a picker; a pick sets `key` of shape/text `tag` (alpha kept).
    fn swatch(&mut self, tag: u64, key: String, color: &str, edited: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let id = format!("{tag}:{key}");
        let rgb = color.get(0..7).unwrap_or("#000000").to_string();
        let fresh = !matches!(self.colors.get(&id), Some((c, ..)) if *c == rgb);
        if fresh {
            let state = cx.new(|cx| ColorPickerState::new(window, cx).default_value(hsla_of(&rgb)));
            let k = key.clone();
            let sub = cx.subscribe(&state, move |this, _, e: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(c)) = e else { return };
                let c = Rgba::from(*c);
                let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                this.set_color(tag, &k, Some([b(c.r), b(c.g), b(c.b)]), None, cx);
            });
            self.colors.insert(id.clone(), (rgb, state, sub));
        }
        let state = self.colors[&id].1.clone();
        div().rounded_sm().when(edited, |d| d.border_1().border_color(theme.warning)).child(ColorPicker::new(&state).xsmall()).into_any_element()
    }

    /// SVG / PNG (at the chosen scale) of a character (a sprite: its first frame).
    fn export_row(&mut self, c: &J, cx: &mut Context<Self>) -> AnyElement {
        let id = c["id"].as_u64().unwrap_or(0);
        let target = if c["kind"] == "sprite" { json!({ "kind": "frame", "id": id, "frame": 0 }) } else { json!({ "kind": "character", "id": id }) };
        let (t1, t2) = (target.clone(), target);
        let scale = self.png_scale;
        let me = cx.entity().downgrade();
        self.prop_row(
            "export",
            h_flex()
                .gap_1()
                .child(Button::new("cx-svg").xsmall().ghost().icon(Icon::new(IconName::Download)).label("SVG").on_click(cx.listener(move |this, _, _, cx| this.export_svg(t1.clone(), cx))))
                .child(Button::new("cx-png").xsmall().ghost().icon(Icon::new(IconName::Download)).label("PNG").on_click(cx.listener(move |this, _, _, cx| this.export_png(t2.clone(), scale, cx))))
                .child(Button::new("cx-scale").xsmall().ghost().label(format!("{scale}×")).tooltip("PNG scale").dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for s in [1u32, 2, 4] {
                        let me = me.clone();
                        menu = menu.item(PopupMenuItem::new(format!("{s}×")).checked(s == scale).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| {
                                this.png_scale = s;
                                cx.notify();
                            });
                        }));
                    }
                    menu
                })),
            cx,
        )
        .into_any_element()
    }

    fn detail_for(&mut self, id: u64, cx: &mut Context<Self>) -> Option<Result<J, String>> {
        match &self.detail {
            Some((i, v, d)) if *i == id && *v == self.version => return Some(d.clone()),
            Some((i, v, _)) if *i == id && *v == u64::MAX - self.version => return None, // in flight
            _ => {}
        }
        self.detail = Some((id, u64::MAX - self.version, Err(String::new())));
        let version = self.version;
        let call = self.backend.call("swf.character", json!({ "path": self.path, "index": self.index, "id": id, "edits": self.pending() }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.version == version {
                    this.detail = Some((id, version, r.map(|r| r.json)));
                    cx.notify();
                }
            });
        })
        .detach();
        None
    }

    pub(super) fn render_character_editor(&mut self, c: &J, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let id = c["id"].as_u64().unwrap_or(0);
        let kind = c["kind"].as_str().unwrap_or("").to_string();
        let tag = c["tag"].as_u64();
        let mut sec = v_flex().gap_1p5();
        if kind == "sprite" {
            sec = sec.child(self.export_row(c, cx));
            let frames = c["frames"].as_u64().unwrap_or(1);
            if frames > 1 {
                let scale = self.png_scale;
                sec = sec.child(Button::new("cx-frames").xsmall().ghost().icon(Icon::new(IconName::Download)).label(format!("All {frames} frames as PNGs…")).tooltip("Every frame's layout as numbered PNGs, all with the same area").on_click(cx.listener(move |this, _, _, cx| this.export_frames(id, scale, cx))));
            }
            return sec.into_any_element();
        }
        if !matches!(kind.as_str(), "shape" | "text" | "edittext" | "font" | "sound" | "binary") {
            if matches!(kind.as_str(), "morph" | "button" | "image" | "subimage" | "bitmap") {
                sec = sec.child(self.export_row(c, cx));
            }
            return sec.into_any_element();
        }
        let detail = match self.detail_for(id, cx) {
            None => return sec.child(div().opacity(0.5).child("Reading…")).into_any_element(),
            Some(Err(e)) => return sec.child(div().text_color(theme.danger).child(e)).into_any_element(),
            Some(Ok(d)) => d,
        };
        let Some(tag) = tag else { return sec.into_any_element() };
        match detail["kind"].as_str().unwrap_or("") {
            "shape" => {
                if self.is_edited(&format!("shapeSvg|{tag}")) {
                    sec = sec.child(div().text_color(theme.warning).child("Redrawn from an SVG (pending)."));
                }
                let alpha = detail["alpha"].as_bool() == Some(true);
                let colors = detail["colors"].as_array().cloned().unwrap_or_default();
                let lines = detail["lines"].as_array().cloned().unwrap_or_default();
                if !colors.is_empty() {
                    sec = sec.child(div().opacity(0.6).child("colours"));
                    for col in &colors {
                        let key = col["key"].as_str().unwrap_or("").to_string();
                        let color = col["color"].as_str().unwrap_or("#000000ff").to_string();
                        let parts: Vec<&str> = key.split(':').collect();
                        let label = if key.starts_with("stop:") { format!("stop {}", parts.get(3).unwrap_or(&"?")) } else { format!("fill {}", parts.get(2).and_then(|n| n.parse::<u64>().ok()).map(|n| n + 1).unwrap_or(0)) };
                        let edited = self.is_edited(&format!("shapeColor|{tag}|{key}"));
                        let mut row = h_flex().gap_1().child(div().w(px(64.0)).font_family("Menlo").opacity(0.5).overflow_hidden().text_ellipsis().child(label)).child(self.swatch(tag, key.clone(), &color, edited, window, cx)).child(self.field(format!("col:{tag}:{key}"), color.get(0..7).unwrap_or("").to_string(), 80.0, cx));
                        if alpha {
                            row = row.child(self.field(format!("alpha:{tag}:{key}"), alpha_of(&color).to_string(), 44.0, cx)).child(div().opacity(0.5).child("%"));
                        }
                        sec = sec.child(row);
                    }
                }
                if !lines.is_empty() {
                    sec = sec.child(div().opacity(0.6).child("lines"));
                    for l in &lines {
                        let key = l["key"].as_str().unwrap_or("").to_string();
                        let ckey = l["colorKey"].as_str().unwrap_or("").to_string();
                        let color = l["color"].as_str().unwrap_or("#000000ff").to_string();
                        let edited = self.is_edited(&format!("shapeColor|{tag}|{key}"));
                        let mut row = h_flex().gap_1().child(self.field(format!("lw:{tag}:{key}"), l["width"].as_f64().map(|w| w.to_string()).unwrap_or_default(), 56.0, cx)).child(div().opacity(0.5).child("px")).child(self.swatch(tag, ckey.clone(), &color, edited, window, cx));
                        if alpha {
                            row = row.child(self.field(format!("alpha:{tag}:{ckey}"), alpha_of(&color).to_string(), 44.0, cx)).child(div().opacity(0.5).child("%"));
                        }
                        sec = sec.child(row);
                    }
                }
                if colors.is_empty() && lines.is_empty() {
                    sec = sec.child(div().opacity(0.6).child(if detail["bitmapFills"].as_u64().unwrap_or(0) > 0 { "Draws an image: replace the picture through the image." } else { "No colours." }));
                }
                if !alpha {
                    sec = sec.child(div().opacity(0.5).child(format!("{}: colours have no alpha.", detail["tagName"].as_str().unwrap_or(""))));
                }
                let fit = self.fit_svg;
                sec = sec.child(
                    h_flex()
                        .gap_2()
                        .child(Button::new("cx-svg-replace").xsmall().icon(Icon::new(IconName::Replace)).label("Replace with SVG…").tooltip("Redraw the shape from an SVG (paths, gradients, strokes; texts and images inside it are left out)").on_click(cx.listener(move |_, _, _, cx| {
                            let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Replace the shape with an SVG".into()) });
                            cx.spawn(async move |this, cx| {
                                let Ok(Ok(Some(mut f))) = files.await else { return };
                                let Some(f) = f.pop() else { return };
                                let r = std::fs::read_to_string(&f);
                                let _ = this.update(cx, |this, cx| match r {
                                    Ok(svg) => this.replace_shape(tag, svg, this.fit_svg, cx),
                                    Err(e) => this.toast(super::Tone::Error, format!("Could not read the SVG: {e}"), cx),
                                });
                            })
                            .detach();
                        })))
                        .child(Checkbox::new("cx-fit").label("fit to bounds").checked(fit).tooltip("Scale the SVG into the shape's current bounds (else one SVG unit per pixel from the shape's origin)").on_click(cx.listener(|this, v: &bool, _, cx| {
                            this.fit_svg = *v;
                            cx.notify();
                        }))),
                );
                sec = sec.child(self.export_row(c, cx));
            }
            "text" => {
                for (i, r) in detail["records"].as_array().cloned().unwrap_or_default().iter().enumerate() {
                    let color = r["color"].as_str().unwrap_or("#000000ff").to_string();
                    let edited = self.is_edited(&format!("staticText|{tag}|{i}"));
                    sec = sec
                        .child(div().opacity(0.6).child(format!("record {} · {} {}px", i + 1, r["fontName"].as_str().map(str::to_string).unwrap_or_else(|| format!("font {}", r["font"])), r["height"])))
                        .child(h_flex().gap_1().child(self.field(format!("st:{tag}:{i}"), r["text"].as_str().unwrap_or("").to_string(), 200.0, cx)).child(self.swatch(tag, format!("record:{i}"), &color, edited, window, cx)));
                }
                sec = sec.child(div().opacity(0.5).child("Characters come from the record's font: add missing ones to the font first.")).child(self.export_row(c, cx));
            }
            "edittext" => {
                let p = detail["props"].clone();
                let edited = self.is_edited(&format!("editText|{tag}"));
                let fonts = detail["fonts"].as_array().cloned().unwrap_or_default();
                let font_label = match (p["font"].as_u64(), p["fontClass"].as_str()) {
                    (Some(f), _) => fonts.iter().find(|x| x["id"].as_u64() == Some(f)).map(|x| format!("{} · {f}", x["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("font {f}")))).unwrap_or_else(|| format!("font {f}")),
                    (None, Some(cl)) => format!("{cl} (class)"),
                    _ => "(none)".into(),
                };
                let me = cx.entity().downgrade();
                let class = p["fontClass"].as_str().map(str::to_string);
                let font_menu = Button::new("cx-font").xsmall().ghost().label(font_label).dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    if let Some(cl) = class.clone() {
                        let me = me.clone();
                        menu = menu.item(PopupMenuItem::new(format!("{cl} (class)")).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.set_edit_text(tag, json!({ "fontClass": cl }), cx));
                        }));
                    }
                    for f in &fonts {
                        let fid = f["id"].as_u64().unwrap_or(0);
                        let me = me.clone();
                        let label = format!("{}{} · {fid}", f["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("font {fid}")), if f["imported"].as_bool() == Some(true) { " (imported)" } else { "" });
                        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.set_edit_text(tag, json!({ "font": fid }), cx));
                        }));
                    }
                    menu
                });
                let me = cx.entity().downgrade();
                let align = p["align"].as_str().unwrap_or("left").to_string();
                let align_menu = Button::new("cx-align").xsmall().ghost().label(align.clone()).dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for a in ["left", "center", "right", "justify"] {
                        let me = me.clone();
                        menu = menu.item(PopupMenuItem::new(a).checked(a == align).on_click(move |_, _, cx| {
                            let _ = me.update(cx, |this, cx| this.set_edit_text(tag, json!({ "align": a }), cx));
                        }));
                    }
                    menu
                });
                let color = p["color"].as_str().unwrap_or("#000000ff").to_string();
                let num = |v: &J| v.as_f64().map(|f| super::geom::fmt(f, 2)).unwrap_or_default();
                let b = &p["bounds"];
                let mut grid = v_flex().gap_1().when(edited, |d| d.border_l_2().border_color(theme.warning).pl_1());
                grid = grid
                    .child(self.prop_row("font", font_menu, cx))
                    .child(self.prop_row("size", h_flex().gap_1().child(self.field(format!("et:{tag}:height"), num(&p["height"]), 70.0, cx)).child("px"), cx))
                    .child({
                        let sw = self.swatch(tag, "editText".into(), &color, edited, window, cx);
                        let al = self.field(format!("alpha:{tag}:editText"), alpha_of(&color).to_string(), 44.0, cx);
                        self.prop_row("colour", h_flex().gap_1().child(sw).child(al).child("%"), cx)
                    })
                    .child(self.prop_row("align", align_menu, cx))
                    .child(self.prop_row("margins", h_flex().gap_1().child(self.field(format!("et:{tag}:leftMargin"), num(&p["leftMargin"]), 52.0, cx)).child(self.field(format!("et:{tag}:rightMargin"), num(&p["rightMargin"]), 52.0, cx)), cx))
                    .child(self.prop_row("indent", self.field(format!("et:{tag}:indent"), num(&p["indent"]), 70.0, cx), cx))
                    .child(self.prop_row("leading", self.field(format!("et:{tag}:leading"), num(&p["leading"]), 70.0, cx), cx))
                    .child(self.prop_row("max length", self.field(format!("et:{tag}:maxLength"), p["maxLength"].as_u64().map(|m| m.to_string()).unwrap_or_default(), 70.0, cx), cx))
                    .child(self.prop_row("variable", self.field(format!("et:{tag}:variable"), p["variable"].as_str().unwrap_or("").to_string(), 160.0, cx), cx))
                    .child(self.prop_row("box", h_flex().gap_1().child(self.field(format!("et:{tag}:bx"), num(&b["x"]), 46.0, cx)).child(self.field(format!("et:{tag}:by"), num(&b["y"]), 46.0, cx)).child(self.field(format!("et:{tag}:bw"), num(&b["w"]), 46.0, cx)).child(self.field(format!("et:{tag}:bh"), num(&b["h"]), 46.0, cx)), cx));
                let mut flags = h_flex().gap_3().flex_wrap();
                for (k, label) in FLAGS {
                    let on = p[*k].as_bool() == Some(true);
                    flags = flags.child(Checkbox::new(*k).label(*label).checked(on).on_click(cx.listener(move |this, v: &bool, _, cx| this.set_edit_text(tag, json!({ *k: *v }), cx))));
                }
                sec = sec.child(grid).child(flags);
            }
            "font" => {
                sec = sec
                    .child(self.prop_row("name", div().font_family("Menlo").child(format!("{}{}{}", detail["name"].as_str().unwrap_or(""), if detail["bold"].as_bool() == Some(true) { " bold" } else { "" }, if detail["italic"].as_bool() == Some(true) { " italic" } else { "" })), cx))
                    .child(self.prop_row("glyphs", div().font_family("Menlo").child(format!("{}{}{}", detail["glyphs"], if detail["layout"].as_bool() == Some(true) { "" } else { " · no layout" }, if detail["alignZones"].as_bool() == Some(true) { " · align zones" } else { "" })), cx))
                    .child(div().id("cx-chars").max_h(px(96.0)).overflow_y_scroll().p_1().rounded_sm().bg(theme.secondary).font_family("Menlo").child(detail["chars"].as_str().filter(|s| !s.is_empty()).unwrap_or("(device font: no glyphs)").to_string()));
                if detail["editable"].as_bool() == Some(true) {
                    sec = sec.child(div().opacity(0.6).child("add characters"));
                    if let Some(p) = self.edit(&format!("fontGlyphs|{tag}")) {
                        let chars = p["chars"].as_str().unwrap_or("");
                        let file = p["file"].as_str().unwrap_or("");
                        sec = sec.child(div().text_color(theme.warning).child(format!("Pending: “{}{}” from {}", chars.chars().take(60).collect::<String>(), if chars.chars().count() > 60 { "…" } else { "" }, file.rsplit(['/', '\\']).next().unwrap_or(file))));
                    }
                    let file = self.glyph_file.clone();
                    sec = sec
                        .child(h_flex().gap_1().child(div().flex_1().min_w_0().font_family("Menlo").overflow_hidden().text_ellipsis().child(file.as_deref().map(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).to_string()).unwrap_or_else(|| "from file…".into()))).child(Button::new("cx-font-file").xsmall().label("Font file…").on_click(cx.listener(|_, _, _, cx| {
                            let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Font with the characters (TTF, OTF)".into()) });
                            cx.spawn(async move |this, cx| {
                                let Ok(Ok(Some(mut f))) = files.await else { return };
                                if let Some(f) = f.pop() {
                                    let _ = this.update(cx, |this, cx| {
                                        this.glyph_file = Some(f.to_string_lossy().to_string());
                                        cx.notify();
                                    });
                                }
                            })
                            .detach();
                        }))))
                        .child(gpui_component::input::Input::new(&self.glyph_chars).xsmall());
                    let mut pre = h_flex().gap_1().flex_wrap();
                    for (k, (name, chars)) in presets().into_iter().enumerate() {
                        pre = pre.child(Button::new(("cx-preset", k)).xsmall().ghost().label(format!("+ {name}")).on_click(cx.listener(move |this, _, window, cx| {
                            let now = this.glyph_chars.read(cx).value().to_string();
                            let next = now + &chars;
                            this.glyph_chars.update(cx, |s, cx| s.set_value(next, window, cx));
                        })));
                    }
                    let ready = file.is_some() && !self.glyph_chars.read(cx).value().is_empty();
                    sec = sec
                        .child(pre)
                        .child(Button::new("cx-add-glyphs").xsmall().icon(Icon::new(IconName::FilePlus)).label("Add to the font").disabled(!ready).on_click(cx.listener(move |this, _, window, cx| {
                            let Some(file) = this.glyph_file.clone() else { return };
                            let chars = this.glyph_chars.read(cx).value().to_string();
                            // Several additions from the same file merge into one edit.
                            let merged = match this.edit(&format!("fontGlyphs|{tag}")) {
                                Some(p) if p["file"].as_str() == Some(&file) => format!("{}{chars}", p["chars"].as_str().unwrap_or("")),
                                _ => chars,
                            };
                            this.set_edit(json!({ "kind": "fontGlyphs", "tag": tag, "file": file, "chars": merged }), cx);
                            this.glyph_chars.update(cx, |s, cx| s.set_value("", window, cx));
                        })))
                        .child(div().opacity(0.5).child("Glyphs are inserted in character order; static texts using the font follow. The file's outlines and advances are scaled to the font."));
                }
                let none = detail["glyphs"].as_u64().unwrap_or(0) == 0;
                sec = sec.child(self.prop_row("export", Button::new("cx-ttf").xsmall().ghost().icon(Icon::new(IconName::Download)).label("TrueType").disabled(none).on_click(cx.listener(move |this, _, _, cx| this.export_file(id, "ttf", "ttf", cx))), cx));
            }
            "sound" => {
                let ext = detail["ext"].as_str().map(str::to_string);
                sec = sec.child(div().font_family("Menlo").child(format!("{} · {} Hz · {}{}", detail["format"].as_str().unwrap_or(""), detail["rate"], if detail["channels"].as_u64() == Some(2) { "stereo" } else { "mono" }, detail["samples"].as_u64().map(|s| format!(" · {s} samples")).unwrap_or_default()))).child(
                    Button::new("cx-sound").xsmall().ghost().icon(Icon::new(IconName::Download)).label(format!("Export {}", ext.as_deref().map(|e| e.to_ascii_uppercase()).unwrap_or_else(|| "(not supported)".into()))).disabled(ext.is_none()).on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(e) = &ext {
                            this.export_file(id, "sound", e, cx);
                        }
                    })),
                );
            }
            "binary" => {
                sec = sec.child(div().font_family("Menlo").child(format!("{} bytes", detail["bytes"]))).child(Button::new("cx-binary").xsmall().ghost().icon(Icon::new(IconName::Download)).label("Export").on_click(cx.listener(move |this, _, _, cx| this.export_file(id, "binary", "bin", cx))));
            }
            _ => {}
        }
        sec.into_any_element()
    }
}
