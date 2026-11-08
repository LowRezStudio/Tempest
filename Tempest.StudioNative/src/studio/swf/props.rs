//! The properties pane: the selected placement (name, position, scale,
//! rotation, skew, alpha, show/hide, remove) when an object of the timeline
//! is selected, else the open character (kind, linkage, tag, size, what it
//! uses and what uses it, attached tags, an edit-text's initial text, an
//! external image's file and texture, and the character editor). Values
//! commit on Enter as pending edits.

use gpui::{div, img, prelude::*, px, AnyElement, Context, ObjectFit, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::geom::{decompose, fmt, M};
use super::{group_icon, MovieEditor, Node, GROUPS};

impl MovieEditor {
    /// A row of the pane: a label and its value.
    pub(super) fn prop_row(&self, label: &str, value: impl IntoElement, cx: &Context<Self>) -> gpui::Div {
        let theme = cx.theme();
        h_flex().gap_1().items_center().text_xs().child(div().w(px(72.0)).flex_none().text_color(theme.muted_foreground).child(label.to_string())).child(value)
    }

    fn group_rank(&self, id: u64) -> usize {
        let kind = self.kind_of(id);
        GROUPS.iter().position(|(_, _, k)| k.contains(&kind)).unwrap_or(GROUPS.len())
    }

    /// Characters by group (tree order), then named ones A–Z, then the unnamed ones by id.
    fn sorted(&self, ids: &[u64]) -> Vec<u64> {
        let mut ids = ids.to_vec();
        let unnamed = |id: u64| self.char(id).is_none_or(|c| c["names"][0].is_null());
        ids.sort_by(|a, b| self.group_rank(*a).cmp(&self.group_rank(*b)).then(unnamed(*a).cmp(&unnamed(*b))).then_with(|| if unnamed(*a) { std::cmp::Ordering::Equal } else { self.label(*a).to_ascii_lowercase().cmp(&self.label(*b).to_ascii_lowercase()) }).then(a.cmp(b)));
        ids
    }

    fn char_list(&self, title: &str, ids: &[u64], key: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut out = v_flex().gap_0p5().child(h_flex().gap_1().text_xs().text_color(theme.muted_foreground).child(title.to_string()).when(!ids.is_empty(), |d| d.child(div().opacity(0.7).child(ids.len().to_string()))));
        if ids.is_empty() {
            return out.child(div().text_xs().opacity(0.5).child("none")).into_any_element();
        }
        let mut list = v_flex().id(key).max_h(px(220.0)).overflow_y_scroll().rounded_md().border_1().border_color(theme.border);
        for (k, id) in self.sorted(ids).into_iter().enumerate() {
            let named = self.char(id).is_some_and(|c| !c["names"][0].is_null());
            let gid = GROUPS.get(self.group_rank(id)).map(|g| g.0).unwrap_or("others");
            list = list.child(
                h_flex()
                    .id((key, k))
                    .gap_1p5()
                    .px_1p5()
                    .py_0p5()
                    .text_xs()
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.list_hover))
                    .child(Icon::new(group_icon(gid)).xsmall().text_color(theme.muted_foreground))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(self.label(id)))
                    .when(named, |d| d.child(div().font_family("Menlo").opacity(0.4).child(id.to_string())))
                    .child(Icon::new(IconName::ChevronRight).xsmall().opacity(0.3))
                    .on_click(cx.listener(move |this, _, _, cx| this.open(Node::Char(id), cx))),
            );
        }
        out = out.child(list);
        out.into_any_element()
    }

    /// A texture's thumbnail (read once per size; dropped when the package changes).
    pub(super) fn thumb(&mut self, texture: usize, max: u32, cx: &mut Context<Self>) -> Option<std::sync::Arc<gpui::Image>> {
        if let Some(t) = self.thumbs.get(&(texture, max)) {
            return t.clone();
        }
        self.thumbs.insert((texture, max), None);
        let call = self.backend.call("texture.preview", json!({ "path": self.path, "index": texture, "maxSize": max }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(r) = r {
                    if let Some(png) = r.bin(&r.json["png"]) {
                        this.thumbs.insert((texture, max), Some(std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png.to_vec()))));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        None
    }

    /// An external image: its texture's thumbnail and Replace / PNG / DDS / go to the texture.
    pub(super) fn image_card(&mut self, image: &J, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tag = image["tag"].as_u64().unwrap_or(0);
        let file = self.value("image", tag, image["fileName"].as_str().unwrap_or(""));
        let texture = self.texture_for(&file, cx);
        let pic = texture.and_then(|t| self.thumb(t, if compact { 64 } else { 256 }, cx));
        let (w, h) = (image["width"].as_f64().unwrap_or(0.0), image["height"].as_f64().unwrap_or(0.0));
        let picture = div()
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .overflow_hidden()
            .bg(theme.secondary)
            .when(compact, |d| d.size(px(32.0)))
            .when(!compact, |d| d.h(px(128.0)).w_full())
            .child(match (pic, texture) {
                (Some(p), _) => img(p).object_fit(ObjectFit::Contain).size_full().into_any_element(),
                (None, None) => div().text_xs().opacity(0.4).child("?").into_any_element(),
                (None, Some(_)) => div().text_xs().opacity(0.4).child("…").into_any_element(),
            });
        let id = image["tag"].as_u64().unwrap_or(0) as usize;
        let (i1, i2, i3) = (image.clone(), image.clone(), image.clone());
        let missing = texture.is_none();
        let buttons = h_flex()
            .gap_1()
            .flex_wrap()
            .child(Button::new(("img-replace", id)).xsmall().when(compact, |b| b.ghost().icon(Icon::new(IconName::Upload))).when(!compact, |b| b.icon(Icon::new(IconName::Upload)).label("Replace…")).disabled(missing).tooltip(format!("Store a PNG or DDS in the texture (encoded in its format; DXT1 becomes DXT5 when the picture has transparency); the movie keeps drawing it at {w}×{h}")).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(t) = texture {
                    this.replace_image(t, i1.clone(), cx);
                }
            })))
            .child(Button::new(("img-png", id)).xsmall().ghost().icon(Icon::new(IconName::Download)).when(!compact, |b| b.label("PNG")).disabled(missing).tooltip("Export as PNG").on_click(cx.listener(move |this, _, _, cx| {
                if let Some(t) = texture {
                    this.export_image(t, &i2, "png", cx);
                }
            })))
            .when(!compact, |d| {
                d.child(Button::new(("img-dds", id)).xsmall().ghost().icon(Icon::new(IconName::Download)).label("DDS").disabled(missing).on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(t) = texture {
                        this.export_image(t, &i3, "dds", cx);
                    }
                })))
            })
            .child(Button::new(("img-go", id)).xsmall().ghost().icon(Icon::new(IconName::Link)).when(!compact, |b| b.label("Texture")).disabled(missing).tooltip("Go to the texture").on_click(cx.listener(move |this, _, _, cx| {
                if let Some(t) = texture {
                    this.go_to_texture(t, cx);
                }
            })));
        if compact {
            h_flex().gap_1().child(picture).child(buttons).into_any_element()
        } else {
            v_flex().gap_1().child(picture).child(buttons).into_any_element()
        }
    }

    pub(super) fn render_props(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let entry = self.selected_entry();
        let shown_id = entry.as_ref().and_then(|e| e["character"].as_u64()).or(match self.node {
            Node::Char(id) => Some(id),
            _ => None,
        });
        let mut out = v_flex().id("mv-props").size_full().overflow_y_scroll().p_2().gap_3().text_xs().border_l_1().border_color(theme.border);
        if let Some(e) = entry.clone() {
            let d = decompose(&M::from_json(&e["matrix"]));
            let alpha = e["cxform"]["mul"][3].as_f64().unwrap_or(1.0) * 100.0;
            let depth = e["depth"].as_u64().unwrap_or(0);
            let id = e["character"].as_u64().unwrap_or(0);
            let place_code = e["placeCode"].as_u64().unwrap_or(26);
            let (e1, e2, e3, e4) = (e.clone(), e.clone(), e.clone(), e.clone());
            let mut sec = v_flex().gap_1p5().child(
                h_flex()
                    .gap_1()
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Placement"))
                    .child(div().font_family("Menlo").opacity(0.5).child(format!("depth {depth}")))
                    .child(div().flex_1())
                    .when(self.entry_edited(&e), |d| d.child(Button::new("mv-revert-entry").xsmall().ghost().icon(Icon::new(IconName::RotateCcw)).tooltip("Drop this object's pending edits").on_click(cx.listener(move |this, _, _, cx| this.revert_entry(&e1, cx))))),
            );
            if self.kind_of(id) == "sprite" {
                let label = e["name"].as_str().map(str::to_string).unwrap_or_else(|| self.label(id));
                sec = sec.child(Button::new("mv-edit-inside").xsmall().primary().icon(Icon::new(IconName::ArrowRight)).label(format!("Edit inside {label}")).tooltip("Open this clip's own timeline to move what is inside it (double-click on the stage, or Enter)").on_click(cx.listener(move |this, _, _, cx| {
                    this.enter(&e2, cx);
                })));
            }
            let name = if place_code == 4 { div().opacity(0.5).child("(PlaceObject has no name)").into_any_element() } else { self.field("p:name".into(), e["name"].as_str().unwrap_or("").to_string(), 170.0, cx) };
            sec = sec
                .child(self.prop_row("name", name, cx))
                .child(self.prop_row("x", h_flex().gap_1().child(self.field("p:x".into(), fmt(d.x, 2), 120.0, cx)).child("px"), cx))
                .child(self.prop_row("y", h_flex().gap_1().child(self.field("p:y".into(), fmt(d.y, 2), 120.0, cx)).child("px"), cx))
                .child(self.prop_row("scale x", h_flex().gap_1().child(self.field("p:sx".into(), fmt(d.sx * 100.0, 2), 120.0, cx)).child("%"), cx))
                .child(self.prop_row("scale y", h_flex().gap_1().child(self.field("p:sy".into(), fmt(d.sy * 100.0, 2), 120.0, cx)).child("%"), cx))
                .child(div().pl(px(76.0)).child(Checkbox::new("mv-lock").label("keep proportions").checked(self.lock_scale).on_click(cx.listener(|this, v: &bool, _, cx| {
                    this.lock_scale = *v;
                    cx.notify();
                }))))
                .child(self.prop_row("rotation", h_flex().gap_1().child(self.field("p:rot".into(), fmt(d.rot, 2), 120.0, cx)).child("°"), cx))
                .child(self.prop_row("skew", h_flex().gap_1().child(self.field("p:skew".into(), fmt(d.skew, 2), 120.0, cx)).child("°"), cx))
                .child(self.prop_row("alpha", h_flex().gap_1().child(self.field("p:alpha".into(), fmt(alpha, 1), 120.0, cx)).child("%"), cx));
            let confirm = self.confirm_remove;
            sec = sec.child(
                h_flex()
                    .gap_1()
                    .pt_1()
                    .child(Button::new("mv-showhide").xsmall().ghost().icon(Icon::new(if alpha == 0.0 { IconName::Eye } else { IconName::EyeOff })).label(if alpha == 0.0 { "Show" } else { "Hide" }).tooltip(if alpha == 0.0 { "Make it visible again (alpha 100%)" } else { "Hide it (alpha 0)" }).on_click(cx.listener(move |this, _, _, cx| this.set_alpha(&e3, if alpha == 0.0 { 100.0 } else { 0.0 }, cx))))
                    .child({
                        let b = Button::new("mv-remove").xsmall().icon(Icon::new(IconName::Trash)).label(if confirm { "Click again to remove" } else { "Remove" }).tooltip("Delete the tag that places it and every later change of it");
                        (if confirm { b.danger() } else { b.ghost() }).on_click(cx.listener(move |this, _, _, cx| {
                            if this.confirm_remove {
                                this.confirm_remove = false;
                                this.remove_entry(&e4, cx);
                            } else {
                                this.confirm_remove = true;
                                cx.notify();
                            }
                        }))
                    }),
            );
            let placed = e["placedBy"].as_u64().unwrap_or(0);
            let matrix_by = e["matrixBy"].as_u64().unwrap_or(0);
            let tag_name = match place_code {
                70 => "PlaceObject3",
                26 => "PlaceObject2",
                _ => "PlaceObject",
            };
            sec = sec.child(div().font_family("Menlo").opacity(0.5).child(format!(
                "{tag_name} #{placed}{} · since frame {}{}{}{}{}",
                if matrix_by != placed { format!(", moved by #{matrix_by}") } else { String::new() },
                e["since"].as_u64().unwrap_or(0) + 1,
                e["ratio"].as_u64().map(|r| format!(" · ratio {r}")).unwrap_or_default(),
                e["clipDepth"].as_u64().map(|c| format!(" · masks to depth {c}")).unwrap_or_default(),
                if e["extras"].as_bool() == Some(true) { " · filters/blend kept" } else { "" },
                e["className"].as_str().map(|c| format!(" · class {c}")).unwrap_or_default()
            )));
            out = out.child(sec);
        }
        let Some(id) = shown_id else {
            if entry.is_none() {
                out = out.child(div().opacity(0.6).child("Select a character in the tree or an object on the stage."));
            }
            return out.into_any_element();
        };
        let Some(c) = self.char(id).cloned() else { return out.into_any_element() };
        let kind = c["kind"].as_str().unwrap_or("?").to_string();
        let mut sec = v_flex().gap_1p5().child(
            h_flex()
                .gap_1()
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(if kind == "edittext" { "Edit text".to_string() } else { capital(&kind) }))
                .child(div().font_family("Menlo").opacity(0.5).child(format!("id {id}")))
                .when(entry.is_some(), |d| d.child(div().flex_1()).child(Button::new("mv-open-char").xsmall().ghost().label("Open").on_click(cx.listener(move |this, _, _, cx| this.open(Node::Char(id), cx))))),
        );
        let names: Vec<&str> = c["names"].as_array().into_iter().flatten().filter_map(|n| n.as_str()).collect();
        if !names.is_empty() {
            sec = sec.child(self.prop_row("linkage", div().font_family("Menlo").child(names.join(", ")), cx));
        }
        sec = sec.child(self.prop_row("tag", div().font_family("Menlo").child(format!("{}{}", c["tag"].as_u64().map(|t| format!("#{t} ")).unwrap_or_default(), c["tagName"].as_str().unwrap_or(""))), cx));
        if let Some(r) = super::geom::Rect::from_json(&c["rect"]) {
            sec = sec.child(self.prop_row("size", div().font_family("Menlo").child(format!("{} × {} at {}, {}", fmt(r.w, 1), fmt(r.h, 1), fmt(r.x, 1), fmt(r.y, 1))), cx));
        }
        if kind == "sprite" {
            sec = sec.child(self.prop_row("frames", div().font_family("Menlo").child(c["frames"].to_string()), cx));
        }
        if let Some(u) = c["import"]["url"].as_str() {
            sec = sec.child(self.prop_row("from", div().font_family("Menlo").child(u.to_string()), cx));
        }
        if kind == "edittext" {
            if let Some(t) = self.info["texts"].as_array().into_iter().flatten().find(|t| t["id"].as_u64() == Some(id)).cloned() {
                let tag = t["tag"].as_u64().unwrap_or(0);
                let text = self.value("text", tag, t["text"].as_str().unwrap_or(""));
                let label = format!(
                    "initial text · {}{}{}{}",
                    t["variable"].as_str().filter(|v| !v.is_empty()).unwrap_or("no variable"),
                    t["fontClass"].as_str().map(|f| format!(" · {f}")).unwrap_or_default(),
                    t["fontHeight"].as_f64().map(|h| format!(" {h}px")).unwrap_or_default(),
                    if t["html"].as_bool() == Some(true) { " · html" } else { "" }
                );
                sec = sec.child(div().opacity(0.6).child(label)).child(self.textarea(format!("text:{tag}"), text, self.is_edited(&format!("text|{tag}|0")), window, cx));
            }
        }
        if kind == "image" || kind == "subimage" {
            if let Some(im) = self.image_of(id) {
                if kind == "subimage" {
                    sec = sec.child(div().opacity(0.6).child(format!("A crop of image {}: replacing the picture changes the whole atlas.", im["id"].as_u64().unwrap_or(0) & 0xffff)));
                } else {
                    let tag = im["tag"].as_u64().unwrap_or(0);
                    let file = self.value("image", tag, im["fileName"].as_str().unwrap_or(""));
                    sec = sec.child(self.prop_row("file", self.field(format!("image:{tag}"), file, 180.0, cx), cx));
                }
                sec = sec.child(self.image_card(&im, false, cx));
            }
        }
        sec = sec.child(self.render_character_editor(&c, window, cx));
        let deps: Vec<u64> = c["deps"].as_array().into_iter().flatten().filter_map(|d| d.as_u64()).collect();
        let used: Vec<u64> = c["usedBy"].as_array().into_iter().flatten().filter_map(|d| d.as_u64()).collect();
        sec = sec.child(self.char_list("Uses", &deps, "mv-uses", cx)).child(self.char_list("Used by", &used, "mv-usedby", cx));
        let attached: Vec<J> = c["attached"].as_array().cloned().unwrap_or_default();
        if !attached.is_empty() {
            let mut tags = h_flex().gap_1().flex_wrap();
            for (k, a) in attached.iter().enumerate() {
                let init = a["name"] == "DoInitAction";
                tags = tags.child(div().id(("mv-attached", k)).px_1().rounded_sm().bg(theme.secondary).font_family("Menlo").cursor_pointer().hover(|d| d.bg(theme.primary.opacity(0.3))).child(format!("#{} {}", a["tag"], a["name"].as_str().unwrap_or(""))).tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Show in the tag list").build(window, cx)).on_click(cx.listener(move |this, _, _, cx| this.open(if init { Node::Scripts } else { Node::Tags }, cx))));
            }
            sec = sec.child(v_flex().gap_0p5().child(div().opacity(0.6).child("attached tags")).child(tags));
        }
        out.child(sec).into_any_element()
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
