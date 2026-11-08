//! What can be done to the selected export: rename, duplicate, delete,
//! export to a file, replace from a file (by class: textures and meshes are
//! imported, anything else takes a raw payload). Each change is an unsaved
//! edit of the package, undoable; the export table is read again after it.

use std::path::{Path, PathBuf};

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{div, prelude::*, px, Context, PathPromptOptions, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use serde_json::{json, Value as J};

use super::{file_name, Studio, Tone};
use crate::model::Tables;

/// The value of `Studio::editing` while the name is edited.
pub const RENAME: &str = "\u{0}rename";

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Texture,
    Mesh,
    /// Sounds and movies (`media.*`).
    Media,
    Anim,
    Other,
}

fn kind(class: &str) -> Kind {
    let c = class.to_ascii_lowercase();
    if c.contains("texture2d") || c == "lightmaptexture2d" || c == "shadowmaptexture2d" {
        Kind::Texture
    } else if c == "staticmesh" || c == "skeletalmesh" {
        Kind::Mesh
    } else if c == "animsequence" || c == "animset" {
        Kind::Anim
    } else if c == "soundnodewave" || c == "swfmovie" {
        Kind::Media
    } else {
        Kind::Other
    }
}

impl Studio {
    fn selection(&self) -> Option<(String, usize, String, String)> {
        let t = self.open.as_ref()?;
        let i = self.selected_export?;
        let e = t.exports.get(i)?;
        Some((t.path.clone(), i, e.name.clone(), e.class.clone()))
    }

    pub(super) fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, _, name, _)) = self.selection() {
            self.start_edit(RENAME.into(), name, window, cx);
        }
    }

    /// Run a change of the package, then read its tables again and select `select(reply)`.
    fn change(&mut self, method: &'static str, params: J, done: impl FnOnce(&J) -> String + 'static, select: impl FnOnce(&J, usize) -> Option<usize> + 'static, cx: &mut Context<Self>) {
        let Some((path, i, _, _)) = self.selection() else { return };
        let call = self.backend.call(method, params);
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let Ok(tables) = this.update(cx, |this, cx| match &r {
                Ok(r) => {
                    this.dirty.insert(path.clone());
                    let what = done(&r.json);
                    this.toast(Tone::Success, what, cx);
                    Some(this.backend.call("package.tables", json!({ "path": path })))
                }
                Err(e) => {
                    this.toast(Tone::Error, e.clone(), cx);
                    None
                }
            }) else { return };
            let Some(tables) = tables else { return };
            let Ok(t) = tables.await else { return };
            let reply = r.map(|r| r.json).unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                if this.open.as_ref().map(|o| o.path.as_str()) != Some(path.as_str()) {
                    return;
                }
                this.open = Some(Tables::from_json(&path, &t.json));
                this.refresh_rows(cx);
                let n = this.open.as_ref().map(|o| o.exports.len()).unwrap_or(0);
                match select(&reply, i).filter(|&k| k < n) {
                    Some(k) => {
                        this.reveal(k, cx);
                        this.user_select_export(k, cx);
                    }
                    None => {
                        this.clear_selection();
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn rename(&mut self, name: String, cx: &mut Context<Self>) {
        let Some((path, i, old, _)) = self.selection() else { return };
        if name.trim().is_empty() || name == old {
            return;
        }
        let shown = name.clone();
        self.change("object.rename", json!({ "path": path, "index": i, "name": name }), move |_| format!("Renamed {old} to {shown}"), |_, i| Some(i), cx);
    }

    pub(super) fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some((path, i, name, _)) = self.selection() else { return };
        self.change("object.duplicate", json!({ "path": path, "index": i }), move |_| format!("Duplicated {name}"), |r, _| r["index"].as_u64().map(|n| n as usize), cx);
    }

    pub(super) fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, i, name, _)) = self.selection() else { return };
        let me = cx.entity().downgrade();
        super::dialogs::confirm(window, cx, format!("Delete {name}?"), "Its sub-objects go too, and references to them are cleared. Undo (⌘Z) takes it back.", "Delete", true, move |_, cx| {
            let (path, name) = (path.clone(), name.clone());
            let _ = me.update(cx, |this, cx| {
                this.change("object.delete", json!({ "path": path, "indices": [i] }), move |r| format!("Deleted {name} ({} references cleared)", r["cleared"]), |_, _| None, cx);
            });
        });
    }

    /// Export: the options of the kind (format, mip, LOD, every sequence), then where.
    pub(super) fn export_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, i, name, class)) = self.selection() else { return };
        let k = kind(&class);
        let (method, formats): (&'static str, &'static [(&'static str, &'static str)]) = match k {
            Kind::Texture => ("texture.export", &[("png", "PNG image"), ("dds", "DDS (as stored, every mip)")]),
            Kind::Mesh => ("mesh.export", &[("glb", "glTF binary (.glb, Blender)"), ("fbx", "FBX (Maya, 3ds Max)"), ("obj", "OBJ (geometry only)")]),
            Kind::Anim => ("anim.export", &[("glb", "glTF binary (.glb)"), ("fbx", "FBX")]),
            Kind::Media if class.eq_ignore_ascii_case("swfmovie") => ("media.export", &[("gfx", "GFX movie")]),
            Kind::Media => ("media.export", &[("ogg", "Ogg Vorbis")]),
            Kind::Other => ("object.extract", &[("bin", "raw payload (.bin)")]),
        };
        let mips = self.tex.as_ref().filter(|t| t.export == i).map(|t| t.mips.iter().map(|m| format!("{}×{}", m.0, m.1)).collect::<Vec<_>>()).unwrap_or_default();
        let lods = self.mesh_lods();
        let set = class.eq_ignore_ascii_case("animset");
        let opts = Rc::new(RefCell::new(Opts { format: 0, mip: 0, lod: 0, all: set, ..Default::default() }));
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let o = opts.borrow().clone();
            let mut body = v_flex().gap_3();
            body = body.child(choices("Format", formats.iter().map(|f| f.1.to_string()).collect(), o.format, opts.clone(), |o, v| o.format = v, &theme));
            if k == Kind::Texture && mips.len() > 1 && formats[o.format].0 == "png" {
                body = body.child(choices("Size (mip)", mips.clone(), o.mip, opts.clone(), |o, v| o.mip = v, &theme));
            }
            if k == Kind::Mesh && lods > 1 && formats[o.format].0 == "obj" {
                body = body.child(choices("LOD", (0..lods).map(|l| format!("LOD {l}")).collect(), o.lod, opts.clone(), |o, v| o.lod = v, &theme));
            }
            if k == Kind::Anim {
                let flag = opts.clone();
                body = body.child(Checkbox::new("exp-all").label("Every sequence of the animation set (one animation each)").checked(o.all).disabled(set).on_click(move |v, window, _| {
                    flag.borrow_mut().all = *v;
                    window.refresh();
                }));
            }
            if k == Kind::Mesh || k == Kind::Anim {
                body = body.child(div().text_xs().text_color(theme.muted_foreground).child("Textures are embedded (glTF, FBX). glTF is in metres, FBX in centimetres; the skeleton's joints point along the bones so Blender draws limbs."));
            }
            let (me, opts, path, name) = (me.clone(), opts.clone(), path.clone(), name.clone());
            dialog.title(format!("Export {name}")).w(px(520.0)).child(body).footer(super::dialogs::footer("Choose where…", false, true, move |window, cx| {
                window.close_dialog(cx);
                let o = opts.borrow().clone();
                let ext = formats[o.format].0;
                let mut params = json!({ "path": path, "index": i, "mip": o.mip, "lod": o.lod, "all": o.all });
                let target_name = format!("{name}.{ext}");
                let _ = me.update(cx, |_this, cx| {
                    let target = cx.prompt_for_new_path(&dirs_documents(), Some(&target_name));
                    cx.spawn(async move |this, cx| {
                        let Ok(Ok(Some(out))) = target.await else { return };
                        params["out"] = json!(out.to_string_lossy());
                        let Ok(call) = this.update(cx, |this, _| this.backend.call(method, params)) else { return };
                        let r = call.await;
                        let _ = this.update(cx, |this, cx| match r {
                            Ok(r) => {
                                this.toast(Tone::Success, format!("Exported {} ({})", out.display(), crate::model::human_size(r.json["bytes"].as_u64().unwrap_or(0))), cx);
                                for w in r.json["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()).take(3) {
                                    this.toast(Tone::Warning, w.to_string(), cx);
                                }
                            }
                            Err(e) => this.toast(Tone::Error, format!("Export failed: {e}"), cx),
                        });
                    })
                    .detach();
                });
            }))
        });
    }

    /// Replace: the options of the kind (texture encoding, mesh LOD, every sequence), then the file.
    pub(super) fn replace_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, i, name, class)) = self.selection() else { return };
        let k = kind(&class);
        let what = match k {
            Kind::Texture => "a PNG or DDS image",
            Kind::Mesh => "a glTF, FBX or OBJ model",
            Kind::Anim => "a glTF or FBX animation (bones are matched by name)",
            Kind::Media if class.eq_ignore_ascii_case("swfmovie") => "a GFX or SWF movie",
            Kind::Media => "an Ogg, WAV or Wwise .wem sound",
            Kind::Other => "a raw payload (.bin, from Export)",
        };
        let lods = self.mesh_lods();
        let set = class.eq_ignore_ascii_case("animset");
        let opts = Rc::new(RefCell::new(Opts { mips: true, keep_alpha: true, pad: true, all: set, ..Default::default() }));
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let o = opts.borrow().clone();
            let check = |id: &'static str, label: &'static str, on: bool, set: fn(&mut Opts, bool)| {
                let opts = opts.clone();
                Checkbox::new(id).label(label).checked(on).on_click(move |v, window, _| {
                    set(&mut opts.borrow_mut(), *v);
                    window.refresh();
                })
            };
            let mut body = v_flex().gap_3().child(div().text_sm().text_color(theme.muted_foreground).child(format!("{name} becomes {what}. It is an unsaved edit: Undo takes it back.")));
            match k {
                Kind::Texture => {
                    body = body
                        .child(choices("Compression", vec!["Like the original".into(), "DXT1 (no alpha, smallest)".into(), "DXT5 (with alpha)".into(), "Uncompressed (BGRA8)".into()], o.format, opts.clone(), |o, v| o.format = v, &theme))
                        .child(check("imp-mips", "Make every mip (smaller sizes for distance)", o.mips, |o, v| o.mips = v))
                        .child(check("imp-alpha", "Keep transparency (a DXT1 texture becomes DXT5 when the image has alpha)", o.keep_alpha, |o, v| o.keep_alpha = v))
                        .child(check("imp-pad", "Pad sizes that are not a multiple of 4 instead of scaling (UI images)", o.pad, |o, v| o.pad = v));
                }
                Kind::Mesh if lods > 1 => {
                    body = body.child(choices("Into", (0..lods).map(|l| if l == 0 { "LOD 0 (the full model)".to_string() } else { format!("LOD {l}") }).collect(), o.lod, opts.clone(), |o, v| o.lod = v, &theme));
                }
                Kind::Anim => {
                    body = body.child(check("imp-all", "Every sequence: each animation of the file goes into the sequence of the same name", o.all, |o, v| o.all = v));
                }
                _ => {}
            }
            let (me, opts, path, name) = (me.clone(), opts.clone(), path.clone(), name.clone());
            dialog.title(format!("Replace {name}")).w(px(560.0)).child(body).footer(super::dialogs::footer("Choose the file…", false, true, move |window, cx| {
                window.close_dialog(cx);
                let o = opts.borrow().clone();
                let (path, name) = (path.clone(), name.clone());
                let _ = me.update(cx, |_this, cx| {
                    let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(format!("Replace {name} with {what}").into()) });
                    cx.spawn(async move |this, cx| {
                        let Ok(Ok(Some(mut picked))) = files.await else { return };
                        let Some(file) = picked.pop() else { return };
                        let file_s = file.to_string_lossy().to_string();
                        let format = ["", "PF_DXT1", "PF_DXT5", "PF_A8R8G8B8"][o.format.min(3)];
                        let (method, params) = match k {
                            Kind::Texture => ("texture.import", json!({ "path": path, "index": i, "file": file_s, "keepAlpha": o.keep_alpha, "mips": o.mips, "padBlocks": o.pad, "format": format })),
                            Kind::Mesh => ("mesh.import", json!({ "path": path, "index": i, "file": file_s, "lod": o.lod })),
                            Kind::Anim => ("anim.import", json!({ "path": path, "index": i, "file": file_s, "all": o.all })),
                            Kind::Media => ("media.import", json!({ "path": path, "index": i, "file": file_s })),
                            Kind::Other => ("object.replaceRaw", json!({ "path": path, "index": i, "file": file_s })),
                        };
                        let shown = file_name(&file_s);
                        // A file with several animations: which one goes in.
                        if k == Kind::Anim && !o.all {
                            let Ok(files) = this.update(cx, |this, _| this.backend.call("anim.files", json!({ "file": file_s }))) else { return };
                            let names: Vec<String> = files.await.ok().map(|r| r.json["animations"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(str::to_string)).collect()).unwrap_or_default();
                            if names.len() > 1 {
                                let _ = this.update(cx, |this, cx| {
                                    this.pending_anim_pick = Some((names, params, name.clone(), shown.clone()));
                                    cx.notify();
                                });
                                return;
                            }
                        }
                        let _ = this.update(cx, |this, cx| this.change(method, params, move |r| {
                            let warn = r["warnings"].as_array().and_then(|w| w.first()).and_then(|w| w.as_str()).map(|w| format!(" ({w})")).unwrap_or_default();
                            format!("Replaced {name} with {shown}{warn}")
                        }, |_, i| Some(i), cx));
                    })
                    .detach();
                });
            }))
        });
    }

    /// LODs of the selected mesh (its `LODInfo` entries).
    fn mesh_lods(&self) -> usize {
        self.inspected.as_ref().and_then(|ins| ins.rows.iter().find(|r| r.depth == 0 && r.name.eq_ignore_ascii_case("lodinfo"))).and_then(|r| r.value.split(' ').next()?.parse::<usize>().ok()).unwrap_or(1).max(1)
    }
}

/// The options of an export or a replace.
#[derive(Clone, Default)]
struct Opts {
    format: usize,
    mip: usize,
    lod: usize,
    all: bool,
    mips: bool,
    keep_alpha: bool,
    pad: bool,
}

/// One choice among a few, as a row of buttons.
fn choices(label: &'static str, items: Vec<String>, at: usize, opts: Rc<RefCell<Opts>>, set: fn(&mut Opts, usize), theme: &gpui_component::Theme) -> impl IntoElement {
    let mut row = h_flex().flex_wrap().gap_1();
    for (k, it) in items.into_iter().enumerate() {
        let opts = opts.clone();
        let b = Button::new(SharedString::from(format!("{label}-{k}"))).small().label(it).on_click(move |_, window, _| {
            set(&mut opts.borrow_mut(), k);
            window.refresh();
        });
        row = row.child(if k == at { b.primary() } else { b.ghost() });
    }
    v_flex().gap_1().child(div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(label)).child(row)
}

fn dirs_documents() -> PathBuf {
    std::env::var_os("HOME").map(|h| Path::new(&h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir)
}

impl Studio {
    /// Which animation of the file replaces the sequence (asked when the file has several).
    pub(super) fn ask_anim_pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((names, params, name, shown)) = self.pending_anim_pick.take() else { return };
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let mut list = v_flex().gap_1();
            for (k, n) in names.iter().enumerate() {
                let (me, params, name, shown, n2) = (me.clone(), params.clone(), name.clone(), shown.clone(), n.clone());
                list = list.child(
                    div()
                        .id(("anim-pick", k))
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_sm()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.list_hover))
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let mut params = params.clone();
                            params["animation"] = json!(k);
                            let (name, shown, n2) = (name.clone(), shown.clone(), n2.clone());
                            let _ = me.update(cx, |this, cx| this.change("anim.import", params, move |_| format!("Replaced {name} with {n2} of {shown}"), |_, i| Some(i), cx));
                        })
                        .child(n.clone()),
                );
            }
            dialog.title(format!("Which animation of {shown}?")).w(px(480.0)).child(div().id("anim-pick-list").max_h(px(400.0)).overflow_y_scroll().child(list))
        });
    }
}
