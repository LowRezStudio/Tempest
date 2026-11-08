//! The 3D viewport: a `MeshView` (wgpu, off-screen) drawn into a canvas;
//! drag orbits, the wheel zooms, double-click refits. A frame is drawn only
//! when the size or the camera changes, and the replaced frame is released
//! from GPUI's atlas. The mesh preview (`mesh.preview`'s GLB, LODs,
//! textures, wireframe) and the material preview's sphere both use it.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{canvas, div, prelude::*, px, AnyElement, Bounds, Context, Corners, Hsla, MouseButton, Pixels, Point, RenderImage, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{h_flex, ActiveTheme, Sizable};
use serde_json::json;

use super::Studio;
use crate::mesh_view::{Camera, MeshView};
use crate::particle_view::ParticleScene;

/// What a viewport draws: the mesh view, or the particle scene.
pub trait Scene {
    fn render(&self, width: u32, height: u32) -> Arc<RenderImage>;
    fn camera_mut(&mut self) -> &mut Camera;
    fn radius(&self) -> f32;
    fn clear_mut(&mut self) -> &mut [f64; 3];
    /// Frame what there is to see.
    fn fit(&mut self);
}

impl Scene for MeshView {
    fn render(&self, width: u32, height: u32) -> Arc<RenderImage> {
        MeshView::render(self, width, height)
    }
    fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }
    fn radius(&self) -> f32 {
        self.radius
    }
    fn clear_mut(&mut self) -> &mut [f64; 3] {
        &mut self.clear
    }
    fn fit(&mut self) {
        self.camera = Camera { yaw: 0.6, pitch: 0.25, distance: self.radius * 2.6 };
    }
}

impl Scene for ParticleScene {
    fn render(&self, width: u32, height: u32) -> Arc<RenderImage> {
        ParticleScene::render(self, width, height)
    }
    fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }
    fn radius(&self) -> f32 {
        self.radius
    }
    fn clear_mut(&mut self) -> &mut [f64; 3] {
        &mut self.clear
    }
    fn fit(&mut self) {
        ParticleScene::fit(self)
    }
}

pub struct MeshState<V: Scene = MeshView> {
    pub export: usize,
    pub lod: usize,
    pub view: V,
    /// The frame shown and the device size it was drawn at.
    image: Option<(Arc<RenderImage>, (u32, u32))>,
    /// Frames replaced since the last paint, to release.
    stale: Vec<Arc<RenderImage>>,
    pub dirty: bool,
    drag: Option<Point<Pixels>>,
    pub summary: String,
}

impl<V: Scene> MeshState<V> {
    pub fn new(export: usize, lod: usize, view: V, summary: String) -> MeshState<V> {
        MeshState { export, lod, view, image: None, stale: Vec::new(), dirty: true, drag: None, summary }
    }

    pub fn fit(&mut self) {
        self.view.fit();
        self.dirty = true;
    }
}

impl MeshState {
    pub fn toggle_wireframe(&mut self) {
        self.view.wireframe = !self.view.wireframe;
        self.dirty = true;
    }
}

pub fn is_mesh(class: &str) -> bool {
    matches!(class.to_ascii_lowercase().as_str(), "staticmesh" | "skeletalmesh")
}

/// The interactive view of `state`, `height` tall, with `bar` over its top
/// left corner and `summary` at the bottom.
pub fn viewport<V: Scene + 'static>(id: &'static str, state: Rc<RefCell<MeshState<V>>>, height: f32, bar: Option<AnyElement>, bg: Hsla, muted: Hsla) -> AnyElement {
    let summary = state.borrow().summary.clone();
    // The picture's background follows the theme (sRGB → linear for the clear colour).
    {
        let c = bg.to_rgb();
        let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } as f64;
        let clear = [lin(c.r), lin(c.g), lin(c.b)];
        let mut st = state.borrow_mut();
        if *st.view.clear_mut() != clear {
            *st.view.clear_mut() = clear;
            st.dirty = true;
        }
    }
    let (s1, s2, s3, s4, s5, s6) = (state.clone(), state.clone(), state.clone(), state.clone(), state.clone(), state.clone());
    let view = canvas(
        move |bounds: Bounds<Pixels>, window: &mut Window, _| {
            let scale = window.scale_factor();
            let size = ((f32::from(bounds.size.width) * scale) as u32, (f32::from(bounds.size.height) * scale) as u32);
            let mut st = s1.borrow_mut();
            if st.dirty || st.image.as_ref().map(|(_, s)| *s) != Some(size) {
                let image = st.view.render(size.0, size.1);
                if let Some((old, _)) = st.image.replace((image, size)) {
                    st.stale.push(old);
                }
                st.dirty = false;
            }
            for old in std::mem::take(&mut st.stale) {
                let _ = window.drop_image(old);
            }
            st.image.as_ref().map(|(i, _)| i.clone())
        },
        |bounds, image, window, _| {
            if let Some(image) = image {
                let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
            }
        },
    )
    .size_full();
    div()
        .id(id)
        .w_full()
        .h(px(height))
        .flex_none()
        .relative()
        .rounded_md()
        .overflow_hidden()
        .bg(bg)
        .on_mouse_down(MouseButton::Left, move |e, _, _| s2.borrow_mut().drag = Some(e.position))
        .on_mouse_up(MouseButton::Left, move |_, _, _| s3.borrow_mut().drag = None)
        .on_mouse_move(move |e: &gpui::MouseMoveEvent, window, _| {
            let mut st = s4.borrow_mut();
            let Some(from) = st.drag else { return };
            if !e.dragging() {
                st.drag = None;
                return;
            }
            let d = e.position - from;
            st.drag = Some(e.position);
            let cam = st.view.camera_mut();
            cam.yaw -= f32::from(d.x) * 0.01;
            cam.pitch = (cam.pitch + f32::from(d.y) * 0.01).clamp(-1.5, 1.5);
            st.dirty = true;
            window.refresh();
        })
        .on_scroll(move |e: &gpui::ScrollEvent, window, cx| {
            // The wheel zooms the view, never the pane around it.
            cx.stop_propagation();
            let mut st = s5.borrow_mut();
            let dy = f32::from(e.delta.pixel_delta(px(16.0)).y);
            let r = st.view.radius();
            let cam = st.view.camera_mut();
            cam.distance = (cam.distance * (1.0 - dy * 0.002)).clamp(r * 0.3, r * 20.0);
            st.dirty = true;
            window.refresh();
        })
        .on_click(move |e: &gpui::ClickEvent, window, _| {
            if e.click_count() == 2 {
                s6.borrow_mut().fit();
                window.refresh();
            }
        })
        .child(view)
        .children(bar.map(|b| h_flex().absolute().top(px(6.0)).left(px(6.0)).gap_1().child(b)))
        .child(div().absolute().bottom(px(6.0)).left(px(8.0)).text_xs().text_color(muted).child(summary))
        .into_any_element()
}

impl Studio {
    /// Fetch and upload the mesh of export `i` (the GPU work off the main thread).
    pub(super) fn load_mesh(&mut self, path: String, i: usize, lod: usize, cx: &mut Context<Self>) {
        let call = self.backend.call("mesh.preview", json!({ "path": path, "index": i, "lod": lod }));
        // A LOD switch keeps the camera and the view settings.
        let keep = self.mesh.as_ref().filter(|m| m.borrow().export == i).map(|m| {
            let m = m.borrow();
            (m.view.camera, m.view.textured, m.view.wireframe)
        });
        cx.spawn(async move |this, cx| {
            let Ok(r) = call.await else { return };
            let Some(glb) = r.bin(&r.json["glb"]).map(|b| b.to_vec()) else { return };
            let t = std::time::Instant::now();
            let busy = crate::backend::Busy::new();
            let built = cx.background_executor().spawn(async move { MeshView::new(&glb) }).await;
            let _ = this.update(cx, |this, cx| {
                drop(busy);
                if this.selected_export != Some(i) {
                    return;
                }
                match built {
                    Ok(mut view) => {
                        if let Some((camera, textured, wireframe)) = keep {
                            view.camera = camera;
                            view.textured = textured;
                            view.wireframe = wireframe;
                        }
                        let summary = format!("{} triangles · {} ms · drag to orbit, wheel to zoom, double-click to fit", view.triangles, t.elapsed().as_millis());
                        this.mesh = Some(Rc::new(RefCell::new(MeshState::new(i, lod, view, summary))));
                    }
                    Err(e) => this.set_status(format!("Mesh preview: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_mesh(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let state = self.mesh.clone()?;
        let (lod, textured, wireframe, can_wire, export) = {
            let st = state.borrow();
            (st.lod, st.view.textured, st.view.wireframe, st.view.can_wireframe(), st.export)
        };
        // LODs: the mesh's `LODInfo` entries.
        let lods = self.inspected.as_ref().and_then(|ins| ins.rows.iter().find(|r| r.depth == 0 && r.name.eq_ignore_ascii_case("lodinfo"))).and_then(|r| r.value.split(' ').next()?.parse::<usize>().ok()).unwrap_or(1).max(1);
        let path = self.open.as_ref().map(|t| t.path.clone()).unwrap_or_default();
        let toggle = |id: &'static str, label: &'static str, on: bool| {
            let b = Button::new(id).xsmall().label(label);
            if on { b.primary() } else { b.ghost() }
        };
        let mut bar = h_flex().gap_1();
        if lods > 1 {
            for l in 0..lods.min(6) {
                let path = path.clone();
                bar = bar.child(toggle_lod(l, l == lod).on_click(cx.listener(move |this, _, _, cx| this.load_mesh(path.clone(), export, l, cx))));
            }
        }
        let (t1, t2, t3) = (state.clone(), state.clone(), state.clone());
        bar = bar.child(toggle("textures", "Textures", textured).tooltip("Draw the base colour textures, or flat grey").on_click(move |_, window, _| {
            let mut st = t1.borrow_mut();
            st.view.textured = !st.view.textured;
            st.dirty = true;
            window.refresh();
        }));
        if can_wire {
            bar = bar.child(toggle("wireframe", "Wireframe", wireframe).on_click(move |_, window, _| {
                t2.borrow_mut().toggle_wireframe();
                window.refresh();
            }));
        }
        bar = bar.children(view_toggles(&state));
        bar = bar.child(Button::new("fit").xsmall().ghost().label("Fit").on_click(move |_, window, _| {
            t3.borrow_mut().fit();
            window.refresh();
        }));
        Some(viewport("mesh-view", state, 420.0, Some(bar.into_any_element()), theme.muted, theme.muted_foreground))
    }
}

fn toggle_lod(l: usize, on: bool) -> Button {
    let b = Button::new(("lod", l)).xsmall().label(format!("LOD {l}"));
    if on {
        b.primary()
    } else {
        b.ghost()
    }
}

/// Grid and Skeleton switches of a viewport.
pub fn view_toggles(state: &Rc<RefCell<MeshState>>) -> Vec<AnyElement> {
    let (grid, skel, has) = {
        let st = state.borrow();
        (st.view.show_grid, st.view.show_skeleton, st.view.has_skeleton())
    };
    let toggle = |id: &'static str, label: &'static str, on: bool, s: Rc<RefCell<MeshState>>, f: fn(&mut crate::mesh_view::MeshView)| {
        let b = Button::new(id).xsmall().label(label).on_click(move |_, window, _| {
            let mut st = s.borrow_mut();
            f(&mut st.view);
            st.dirty = true;
            window.refresh();
        });
        (if on { b.primary() } else { b.ghost() }).into_any_element()
    };
    let mut out = vec![toggle("grid", "Grid", grid, state.clone(), |v| v.show_grid = !v.show_grid)];
    if has {
        out.push(toggle("skeleton", "Skeleton", skel, state.clone(), |v| v.show_skeleton = !v.show_skeleton));
    }
    out
}
