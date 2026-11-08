//! The Havok nav mesh of the open map (`map.navmesh`, toolbar Nav mesh):
//! drawn over the scene as layer 2 (faces coloured by region or cluster,
//! every edge with boundaries and user edges highlighted, the jump links as
//! arcs between their two edges), faces or a vertex picked in the view, and
//! the edits (delete faces, cut a region, move a vertex) written back by
//! `map.navmeshEdit` into the HavokNavMeshActor of the persistent level: an
//! unsaved, undoable edit of that package like any other map edit.

use std::collections::HashSet;

use glam::Vec3;
use gpui::{div, prelude::*, px, AnyElement, Context, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Selectable, Sizable};
use serde_json::{json, Value as J};

use super::MapEditor;
use crate::backend::Reply;
use crate::map_render::{rgba, Cv, MapScene, Node, Part};
use crate::studio::Tone;

/// Lift over the level's surfaces, in metres (the faces lie on them).
const LIFT: f32 = 0.04;

/// One nav mesh actor's buffers.
pub struct NavData {
    pub info: J,
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub tri_face: Vec<u32>,
    pub face_cluster: Vec<i32>,
    pub face_region: Vec<i32>,
    pub lines: Vec<[u32; 2]>,
    pub line_kind: Vec<u8>,
    pub links: Vec<Vec3>,
}

fn f32s(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}
fn u32s(b: &[u8]) -> Vec<u32> {
    b.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}
fn i32s(b: &[u8]) -> Vec<i32> {
    b.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

impl NavData {
    fn decode(a: &J, r: &Reply) -> NavData {
        let bin = |k: &str| r.bin(&a[k]).unwrap_or(&[]);
        let v3 = |v: Vec<f32>| v.as_chunks::<3>().0.iter().map(|c| Vec3::new(c[0], c[1], c[2])).collect::<Vec<_>>();
        let mut info = a.clone();
        if let Some(o) = info.as_object_mut() {
            for k in ["positions", "triangles", "triFace", "faceCluster", "faceRegion", "lines", "lineKind", "links"] {
                o.remove(k);
            }
        }
        NavData {
            info,
            positions: v3(f32s(bin("positions"))),
            triangles: u32s(bin("triangles")).as_chunks::<3>().0.iter().map(|c| [c[0], c[1], c[2]]).collect(),
            tri_face: u32s(bin("triFace")),
            face_cluster: i32s(bin("faceCluster")),
            face_region: i32s(bin("faceRegion")),
            lines: u32s(bin("lines")).as_chunks::<2>().0.iter().map(|c| [c[0], c[1]]).collect(),
            line_kind: bin("lineKind").to_vec(),
            links: v3(f32s(bin("links"))),
        }
    }

    /// The vertex of a face nearest to a point.
    fn nearest_vertex(&self, face: u32, p: Vec3) -> Option<u32> {
        let mut best: Option<(u32, f32)> = None;
        for (t, tri) in self.triangles.iter().enumerate() {
            if self.tri_face.get(t) != Some(&face) {
                continue;
            }
            for &v in tri {
                let d = (self.positions[v as usize] - p).length_squared();
                if best.is_none_or(|(_, b)| d < b) {
                    best = Some((v, d));
                }
            }
        }
        best.map(|b| b.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Off,
    Faces,
    Vertex,
}

pub struct Nav {
    pub show: bool,
    pub by_cluster: bool,
    pub pick: Pick,
    pub vertex: Option<u32>,
    pub xray: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub level: Option<String>,
    pub actors: Vec<NavData>,
    pub current: usize,
    pub selected: HashSet<u32>,
    /// Where the picked vertex goes (Unreal units).
    pub target: [f64; 3],
}

impl Nav {
    pub fn new(_window: &mut Window, _cx: &mut Context<MapEditor>) -> Nav {
        Nav { show: false, by_cluster: false, pick: Pick::Off, vertex: None, xray: false, loading: false, error: None, level: None, actors: Vec::new(), current: 0, selected: HashSet::new(), target: [0.0; 3] }
    }

    pub fn reset(&mut self) {
        self.actors.clear();
        self.level = None;
        self.selected.clear();
        self.vertex = None;
        self.current = 0;
        self.error = None;
    }

    pub fn data(&self) -> Option<&NavData> {
        self.actors.get(self.current)
    }
}

/// Golden-ratio hues: neighbouring ids get distant colours.
fn id_colour(id: i32) -> [f32; 4] {
    let h = ((id as f32 * 0.618_034) % 1.0 + 1.0) % 1.0;
    let c = gpui::hsla(h, 0.55, 0.55, 1.0).to_rgb();
    let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    [lin(c.r), lin(c.g), lin(c.b), 0.45]
}

impl MapEditor {
    pub(super) fn toggle_nav(&mut self, cx: &mut Context<Self>) {
        self.nav.show = !self.nav.show;
        if self.nav.show {
            self.load_nav(false, cx);
        }
        self.rebuild_nav();
        cx.notify();
    }

    /// Read the open map's nav mesh (again when one of its levels changed).
    pub(super) fn load_nav(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else { return };
        if !force && self.nav.level.as_deref() == Some(path.as_str()) && !self.nav.actors.is_empty() {
            return;
        }
        self.nav.loading = true;
        self.nav.error = None;
        let call = self.call("map.navmesh", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.nav.loading = false;
                match r {
                    Ok(r) => {
                        let keep = this.nav.level.as_deref() == Some(path.as_str());
                        this.nav.actors = r.json["actors"].as_array().into_iter().flatten().map(|a| NavData::decode(a, &r)).collect();
                        this.nav.level = Some(path.clone());
                        if !keep || this.nav.current >= this.nav.actors.len() {
                            this.nav.current = 0;
                            this.nav.selected.clear();
                        }
                        if this.nav.actors.is_empty() {
                            this.nav.error = Some("This map has no Havok nav mesh.".into());
                        }
                        this.rebuild_nav();
                    }
                    Err(e) => this.nav.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The nav mesh's layer: faces by region or cluster (picked ones highlighted) and every edge.
    pub(super) fn rebuild_nav(&mut self) {
        let mut nodes = Vec::new();
        if let (true, Some(d)) = (self.nav.show, self.nav.data()) {
            let lift = |v: u32| d.positions.get(v as usize).copied().unwrap_or_default() + Vec3::Y * LIFT;
            let mut faces = Vec::with_capacity(d.triangles.len() * 3);
            for (t, tri) in d.triangles.iter().enumerate() {
                let f = d.tri_face.get(t).copied().unwrap_or(0);
                let c = if self.nav.selected.contains(&f) { rgba(0xf9e2af, 0.6) } else { id_colour(if self.nav.by_cluster { d.face_cluster.get(f as usize).copied().unwrap_or(0) } else { d.face_region.get(f as usize).copied().unwrap_or(0) }) };
                for &v in tri {
                    faces.push(Cv::new(lift(v), c));
                }
            }
            let colours = [rgba(0x6c7086, 1.0), rgba(0xf38ba8, 1.0), rgba(0x89dceb, 1.0)];
            let mut lines = Vec::with_capacity(d.lines.len() * 2);
            for (i, l) in d.lines.iter().enumerate() {
                let c = colours[d.line_kind.get(i).copied().unwrap_or(0).min(2) as usize];
                lines.push(Cv::new(lift(l[0]), c));
                lines.push(Cv::new(lift(l[1]), c));
            }
            nodes.push(Node { matrix: glam::Mat4::IDENTITY, visible: true, rooftop: false, tint: None, parts: vec![Part::Fill { verts: faces, brush: false }, Part::Lines { verts: lines, pick: false }] });
        }
        let xray = self.nav.xray;
        self.view.with(|s| {
            s.layers[2] = nodes;
            s.nav_xray = xray;
            s.invalidate();
        });
        self.view.dirty();
    }

    /// The jump links, over everything: both edges and an arc from A's middle to B's.
    pub(super) fn nav_links(&self, out: &mut Vec<Cv>) {
        let (true, Some(d)) = (self.nav.show, self.nav.data()) else { return };
        let c = rgba(0x94e2d5, 1.0);
        for l in d.links.as_chunks::<4>().0.iter() {
            let [a0, a1, b0, b1] = [l[0], l[1], l[2], l[3]].map(|p| p + Vec3::Y * LIFT);
            let (am, bm) = ((a0 + a1) * 0.5, (b0 + b1) * 0.5);
            let mut top = (am + bm) * 0.5;
            top.y = am.y.max(bm.y) + 0.3 * am.distance(bm) + 0.3;
            let q = |t: f32| am * (1.0 - t) * (1.0 - t) + top * 2.0 * t * (1.0 - t) + bm * t * t;
            for k in 0..16 {
                out.push(Cv::new(q(k as f32 / 16.0), c));
                out.push(Cv::new(q((k + 1) as f32 / 16.0), c));
            }
            for p in [a0, a1, b0, b1] {
                out.push(Cv::new(p, c));
            }
        }
    }

    /// A click while picking nav mesh faces or a vertex (true when it was one).
    pub(super) fn nav_click(&mut self, o: Vec3, d: Vec3, add: bool, cx: &mut Context<Self>) -> bool {
        if !self.nav.show || self.nav.pick == Pick::Off {
            return false;
        }
        let Some(data) = self.nav.data() else { return true };
        let lifted: Vec<Vec3> = data.positions.iter().map(|p| *p + Vec3::Y * LIFT).collect();
        let hit = MapScene::pick_triangles(o, d, &lifted, &data.triangles);
        let Some((t, dist)) = hit else {
            if !add {
                self.nav.selected.clear();
                self.nav.vertex = None;
            }
            self.rebuild_nav();
            cx.notify();
            return true;
        };
        let face = data.tri_face.get(t).copied().unwrap_or(0);
        if self.nav.pick == Pick::Vertex {
            let vertex = data.nearest_vertex(face, o + d * dist);
            let at = vertex.map(|v| data.positions[v as usize]);
            self.nav.vertex = vertex;
            if let Some(p) = at {
                // View space (metres, Y up) → Unreal units.
                self.nav.target = [p.x, p.z, p.y].map(|x| (x as f64 * 10000.0).round() / 100.0);
            }
            self.nav.selected = [face].into();
        } else if add {
            if !self.nav.selected.remove(&face) {
                self.nav.selected.insert(face);
            }
        } else {
            self.nav.selected = [face].into();
        }
        self.rebuild_nav();
        cx.notify();
        true
    }

    /// Every face of the regions holding a picked face.
    fn nav_whole_regions(&mut self) {
        let Some(d) = self.nav.data() else { return };
        let regions: HashSet<i32> = self.nav.selected.iter().filter_map(|f| d.face_region.get(*f as usize).copied()).collect();
        self.nav.selected = (0..d.face_region.len() as u32).filter(|f| regions.contains(&d.face_region[*f as usize])).collect();
        self.rebuild_nav();
    }

    pub(crate) fn nav_edit(&mut self, op: &str, args: J, cx: &mut Context<Self>) {
        let (Some(d), Some(level)) = (self.nav.data(), self.nav.level.clone()) else { return };
        let mut params = json!({ "path": level, "index": d.info["index"], "op": op });
        if let Some(o) = args.as_object() {
            for (k, v) in o {
                params[k] = v.clone();
            }
        }
        let call = self.call("map.navmeshEdit", params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        let fresh = NavData::decode(&r.json, &r);
                        let k = this.nav.current;
                        if k < this.nav.actors.len() {
                            this.nav.actors[k] = fresh;
                        }
                        this.nav.selected.clear();
                        this.nav.vertex = None;
                        this.note_edit(&level, cx);
                        this.rebuild_nav();
                        this.toast(Tone::Success, format!("Nav mesh edited: {} faces, {} regions", r.json["faces"], r.json["regions"]), cx);
                    }
                    Err(e) => this.toast(Tone::Error, format!("Nav mesh edit failed: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn commit_nav_field(&mut self, key: &str, value: String, cx: &mut Context<Self>) {
        if let (Ok(k), Ok(n)) = (key.parse::<usize>(), value.trim().parse::<f64>()) {
            if k < 3 {
                self.nav.target[k] = n;
            }
        }
        cx.notify();
    }

    pub(super) fn render_nav_panel(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.nav.show {
            return None;
        }
        let theme = cx.theme().clone();
        let mut p = v_flex()
            .id("nav-panel")
            .absolute()
            .top(px(8.0))
            .right(px(8.0))
            .w(px(256.0))
            .p_2()
            .gap_2()
            .rounded_md()
            .bg(theme.background.opacity(0.92))
            .shadow_md()
            .text_xs()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(h_flex().gap_2().child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Havok nav mesh")).when(self.nav.loading, |d| d.child(div().text_color(theme.muted_foreground).child("reading…"))).child(div().flex_1()).child(Button::new("nav-close").xsmall().ghost().label("✕").tooltip("Hide the nav mesh").on_click(cx.listener(|this, _, _, cx| this.toggle_nav(cx)))));
        if let Some(e) = &self.nav.error {
            return Some(p.child(div().text_color(theme.muted_foreground).child(e.clone())).into_any_element());
        }
        let Some(d) = self.nav.data() else { return Some(p.into_any_element()) };
        let i = &d.info;
        if self.nav.actors.len() > 1 {
            let me = cx.entity().downgrade();
            let items: Vec<(usize, String)> = self.nav.actors.iter().enumerate().map(|(k, a)| (k, format!("{} ({} faces)", a.info["path"].as_str().unwrap_or("").rsplit('.').next().unwrap_or(""), a.info["faces"]))).collect();
            let label = items.get(self.nav.current).map(|x| x.1.clone()).unwrap_or_default();
            p = p.child(Button::new("nav-actor").xsmall().label(label).dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                for (k, l) in &items {
                    let (me, k) = (me.clone(), *k);
                    menu = menu.item(PopupMenuItem::new(l.clone()).on_click(move |_, _, cx| {
                        let _ = me.update(cx, |this, cx| {
                            this.nav.current = k;
                            this.nav.selected.clear();
                            this.nav.vertex = None;
                            this.rebuild_nav();
                            cx.notify();
                        });
                    }));
                }
                menu
            }));
        }
        let regions_picked: HashSet<i32> = self.nav.selected.iter().filter_map(|f| d.face_region.get(*f as usize).copied()).collect();
        p = p.child(
            div()
                .grid()
                .grid_cols(2)
                .gap_x_2()
                .font_family("Menlo")
                .text_color(theme.muted_foreground)
                .child(format!("{} faces", i["faces"]))
                .child(format!("{} vertices", i["vertices"]))
                .child(format!("{} edges", i["edges"]))
                .child(format!("{} boundary", i["boundary"]))
                .child(format!("{} regions", i["regions"]))
                .child(format!("{} clusters", i["clusters"]))
                .child(format!("{} jump links", i["linkInfo"].as_array().map(|a| a.len()).unwrap_or(0)))
                .child(format!("erosion {}", i["erosionRadius"])),
        );
        let me = cx.entity().downgrade();
        p = p
            .child(
                h_flex()
                    .gap_2()
                    .child("Colour")
                    .child(Button::new("nav-colour").xsmall().label(if self.nav.by_cluster { "by cluster" } else { "by region" }).dropdown_menu(move |menu, _, _| {
                        let (m1, m2) = (me.clone(), me.clone());
                        menu.item(PopupMenuItem::new("by region").on_click(move |_, _, cx| {
                            let _ = m1.update(cx, |this, cx| {
                                this.nav.by_cluster = false;
                                this.rebuild_nav();
                                cx.notify();
                            });
                        }))
                        .item(PopupMenuItem::new("by cluster").on_click(move |_, _, cx| {
                            let _ = m2.update(cx, |this, cx| {
                                this.nav.by_cluster = true;
                                this.rebuild_nav();
                                cx.notify();
                            });
                        }))
                    }))
                    .child(Checkbox::new("nav-xray").label("X-ray").checked(self.nav.xray).tooltip("Draw the nav mesh through the level").on_click(cx.listener(|this, v: &bool, _, cx| {
                        this.nav.xray = *v;
                        this.rebuild_nav();
                        cx.notify();
                    }))),
            )
            .child(h_flex().gap_2().text_color(theme.muted_foreground).children([(0xf38ba8, "boundary"), (0x89dceb, "user edges"), (0x94e2d5, "jump links")].map(|(c, t)| h_flex().gap_1().child(div().w(px(12.0)).h(px(2.0)).bg(super::hsla(c))).child(t))));
        let mut modes = h_flex().gap_0p5();
        for (m, id, label, tip) in [(Pick::Off, "nav-off", "Actors", "Clicks select actors"), (Pick::Faces, "nav-faces", "Faces", "Clicks pick nav mesh faces (Shift adds)"), (Pick::Vertex, "nav-vertex", "Vertex", "Clicks pick the vertex of the face nearest the click")] {
            modes = modes.child(Button::new(id).xsmall().flex_1().label(label).tooltip(tip).selected(self.nav.pick == m).on_click(cx.listener(move |this, _, _, cx| {
                this.nav.pick = m;
                cx.notify();
            })));
        }
        p = p.child(modes);
        let none = self.nav.selected.is_empty();
        match self.nav.pick {
            Pick::Faces => {
                p = p.child(div().text_color(theme.muted_foreground).child(format!("{} faces picked{}", self.nav.selected.len(), if none { String::new() } else { format!(", {} regions", regions_picked.len()) }))).child(
                    h_flex()
                        .flex_wrap()
                        .gap_1()
                        .child(Button::new("nav-regions").xsmall().label("Whole regions").disabled(none).tooltip("Pick every face of the picked faces' regions").on_click(cx.listener(|this, _, _, cx| {
                            this.nav_whole_regions();
                            cx.notify();
                        })))
                        .child(Button::new("nav-clear").xsmall().label("Clear").disabled(none).on_click(cx.listener(|this, _, _, cx| {
                            this.nav.selected.clear();
                            this.nav.vertex = None;
                            this.rebuild_nav();
                            cx.notify();
                        })))
                        .child(Button::new("nav-delete").xsmall().danger().label("Delete faces").disabled(none).tooltip("Delete the picked faces: their neighbours' edges become boundaries, jump links to them go").on_click(cx.listener(|this, _, _, cx| {
                            let faces: Vec<u32> = this.nav.selected.iter().copied().collect();
                            this.nav_edit("deleteFaces", json!({ "faces": faces }), cx);
                        })))
                        .child(Button::new("nav-cut").xsmall().danger().label("Cut region").disabled(none).tooltip("Delete every face connected to the picked ones").on_click(cx.listener(|this, _, _, cx| {
                            let faces: Vec<u32> = this.nav.selected.iter().copied().collect();
                            this.nav_edit("deleteRegion", json!({ "faces": faces }), cx);
                        }))),
                );
            }
            Pick::Vertex => match self.nav.vertex {
                Some(v) => {
                    let t = self.nav.target;
                    p = p
                        .child(div().text_color(theme.muted_foreground).child(format!("Vertex {v} (Unreal units)")))
                        .child(h_flex().gap_1().child(self.field("nav:0", format!("{}", t[0]), 74.0, cx)).child(self.field("nav:1", format!("{}", t[1]), 74.0, cx)).child(self.field("nav:2", format!("{}", t[2]), 74.0, cx)))
                        .child(Button::new("nav-move").xsmall().w_full().label("Move the vertex").on_click(cx.listener(move |this, _, _, cx| {
                            let t = this.nav.target;
                            this.nav_edit("moveVertices", json!({ "moves": [[v, t[0] / 100.0, t[2] / 100.0, t[1] / 100.0]] }), cx);
                        })));
                }
                None => p = p.child(div().text_color(theme.muted_foreground).child("Click a face to pick its nearest vertex.")),
            },
            Pick::Off => {}
        }
        Some(p.child(div().text_color(theme.muted_foreground).opacity(0.7).child("Edits rebuild the face AABB tree and drop the clearance cache seeding; the cluster graph and the baked game data are left as they are. Undo with ⌘Z, save the level to keep them.")).into_any_element())
    }
}
