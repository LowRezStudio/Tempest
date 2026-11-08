//! The package-level questions: confirmations, New package, Add to the
//! package (from a file, a copy, a group, a material instance), Copy to,
//! Replace with, Export a copy, Discard, Check, Find and replace, Batch
//! import. Each change is an unsaved, undoable edit of the package.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{div, prelude::*, px, App, Context, Entity, PathPromptOptions, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use serde_json::{json, Value as J};

use super::{file_name, Studio, Tone};

/// A question with one action: `on_ok` runs when it is chosen.
pub fn confirm(window: &mut Window, cx: &mut App, title: impl Into<SharedString>, body: impl Into<SharedString>, ok: &'static str, danger: bool, on_ok: impl Fn(&mut Window, &mut App) + 'static) {
    let (title, body) = (title.into(), body.into());
    let on_ok = Rc::new(on_ok);
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme().clone();
        let on_ok = on_ok.clone();
        dialog.title(title.clone()).w(px(520.0)).child(div().text_sm().text_color(theme.muted_foreground).child(body.clone())).footer(footer(ok, danger, true, move |window, cx| {
            window.close_dialog(cx);
            on_ok(window, cx);
        }))
    });
}

/// Cancel and the action button (probed `dlg-ok` / `dlg-cancel` for the driver).
pub fn footer(ok: &'static str, danger: bool, enabled: bool, on_ok: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_end()
        .gap_2()
        .child(div().relative().child(Button::new("dlg-cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx))).child(crate::driver::probe("dlg-cancel")))
        .child(
            div()
                .relative()
                .child({
                    let b = Button::new("dlg-ok").label(ok).disabled(!enabled).on_click(move |_, window, cx| on_ok(window, cx));
                    if danger { b.danger() } else { b.primary() }
                })
                .child(crate::driver::probe("dlg-ok")),
        )
}

fn field(label: &'static str, input: &Entity<InputState>, cx: &App) -> impl IntoElement {
    let theme = cx.theme().clone();
    v_flex().gap_1().child(div().text_xs().text_color(theme.muted_foreground).child(label)).child(Input::new(input).small())
}

fn new_input(window: &mut Window, cx: &mut App, value: &str, placeholder: &str) -> Entity<InputState> {
    let (value, placeholder) = (value.to_string(), placeholder.to_string());
    cx.new(|cx| {
        let mut s = InputState::new(window, cx).placeholder(placeholder);
        s.set_value(value, window, cx);
        s
    })
}

/// Focus `input` the first time the dialog is built.
fn focus_once(input: &Entity<InputState>) -> impl Fn(&mut Window, &mut App) {
    let input = input.clone();
    let first = std::cell::Cell::new(true);
    move |window, cx| {
        if first.replace(false) {
            input.update(cx, |s, cx| s.focus(window, cx));
        }
    }
}

/// Dev: dialog steps of the test driver.
pub fn drive(arg: &str, window: &mut Window, cx: &mut App) -> Result<(), String> {
    match arg {
        "close" => {
            window.close_dialog(cx);
            Ok(())
        }
        _ => Err(format!("unknown dialog step {arg}")),
    }
}



impl Studio {


    /// The result of an `object.*` call that made export `index` of `path`: mark, reload, select.
    pub(super) fn added(&mut self, path: &str, index: Option<usize>, what: String, cx: &mut Context<Self>) {
        self.dirty.insert(path.to_string());
        self.toast(Tone::Success, what, cx);
        self.refresh_package(path.to_string(), index, cx);
    }

    // ----- new package ------------------------------------------------------------



    // ----- add to the package ---------------------------------------------------------






    /// The selected export into package `dst` (top level, with what it uses).
    pub(super) fn copy_to(&mut self, dst: String, cx: &mut Context<Self>) {
        let (Some(src), Some(i)) = (self.open.as_ref().map(|t| t.path.clone()), self.selected_export) else { return };
        self.copy_call(src, i, dst, J::from("top"), false, cx);
    }

    fn copy_call(&mut self, src: String, index: usize, dst: String, outer: J, reload: bool, cx: &mut Context<Self>) {
        self.set_status("Copying…", cx);
        let call = self.backend.call("object.copy", json!({ "src": src, "index": index, "dst": dst, "outer": outer, "deep": true }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    let warnings: Vec<String> = r.json["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(|s| s.to_string())).collect();
                    for w in warnings.iter().take(3) {
                        this.toast(Tone::Warning, w.clone(), cx);
                    }
                    let what = format!("Copied as {} into {} (unsaved)", r.json["path"].as_str().unwrap_or_default(), file_name(&dst));
                    if reload {
                        this.added(&dst, r.json["index"].as_u64().map(|i| i as usize), what, cx);
                    } else {
                        this.dirty.insert(dst.clone());
                        this.toast(Tone::Success, what, cx);
                    }
                }
                Err(e) => this.toast(Tone::Error, format!("Could not copy: {e}"), cx),
            });
        })
        .detach();
    }

    pub(super) fn replace_with(&mut self, src: String, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(i)) = (self.open.as_ref().map(|t| t.path.clone()), self.selected_export) else { return };
        let name = self.open.as_ref().map(|t| t.exports[i].name.clone()).unwrap_or_default();
        let me = cx.entity().downgrade();
        confirm(window, cx, format!("Replace {name}?"), format!("{name} becomes a copy of the object picked in {}, keeping its name and place; what it uses is copied along. Undo takes it back.", file_name(&src)), "Replace", false, move |_, cx| {
            let (path, src) = (path.clone(), src.clone());
            let _ = me.update(cx, |this, cx| {
                let call = this.backend.call("object.replaceWith", json!({ "path": path, "index": i, "src": { "path": src, "index": index }, "deep": true }));
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| match r {
                        Ok(r) => this.added(&path, Some(i), format!("{} is now a copy of the picked {}", r.json["path"].as_str().unwrap_or_default(), r.json["class"].as_str().unwrap_or_default()), cx),
                        Err(e) => this.toast(Tone::Error, format!("Could not replace: {e}"), cx),
                    });
                })
                .detach();
            });
        });
    }

    // ----- saving, discarding, checking ---------------------------------------------------

    /// Write the package elsewhere; the game folder is untouched.
    pub(super) fn save_copy(&mut self, path: String, cx: &mut Context<Self>) {
        let dir = std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        let target = cx.prompt_for_new_path(&dir, Some(&file_name(&path)));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(out))) = target.await else { return };
            let Ok(call) = this.update(cx, |this, _| this.backend.call("package.save", json!({ "path": path, "out": out.to_string_lossy() }))) else { return };
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(_) => this.toast(Tone::Success, format!("Wrote a copy to {}; the game folder is unchanged", out.display()), cx),
                Err(e) => this.toast(Tone::Error, format!("Could not write the copy: {e}"), cx),
            });
        })
        .detach();
    }

    pub(super) fn discard(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let me = cx.entity().downgrade();
        confirm(window, cx, format!("Discard the changes to {}?", file_name(&path)), "Every unsaved edit of this package is dropped and it is read again from the game folder. This cannot be undone.", "Discard", true, move |_, cx| {
            let path = path.clone();
            let _ = me.update(cx, |this, cx| {
                let call = this.backend.call("package.discard", json!({ "path": path }));
                cx.spawn(async move |this, cx| {
                    let r = call.await;
                    let _ = this.update(cx, |this, cx| match r {
                        Ok(_) => {
                            this.dirty.remove(&path);
                            this.forget_map_edit(&path, cx);
                            this.toast(Tone::Success, format!("Discarded the changes to {}", file_name(&path)), cx);
                            if let Some(c) = this.cooker.clone() {
                                c.update(cx, |c, cx| c.refresh(cx));
                            }
                            if this.selected_package.as_deref() == Some(path.as_str()) {
                                this.selected_package = None;
                                this.open_package(path.clone(), cx);
                            }
                        }
                        Err(e) => this.toast(Tone::Error, format!("Could not discard: {e}"), cx),
                    });
                })
                .detach();
            });
        });
    }

    pub(super) fn verify(&mut self, path: String, _window: &mut Window, cx: &mut Context<Self>) {
        self.set_status(format!("Checking {}…", file_name(&path)), cx);
        let call = self.backend.call("package.verify", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    let v = &r.json;
                    let ok = v["ok"].as_bool().unwrap_or(false);
                    let text = format!(
                        "{}: {} objects, {} fully understood, {} kept as raw bytes, {} that do not round-trip{}",
                        v["name"].as_str().unwrap_or_default(),
                        v["exports"],
                        v["full"],
                        v["raw"],
                        v["mismatches"],
                        if v["imageIdentical"].as_bool().unwrap_or(false) { "; the rewritten file is identical" } else { "" }
                    );
                    this.toast(if ok { Tone::Success } else { Tone::Warning }, text, cx);
                }
                Err(e) => this.toast(Tone::Error, format!("Could not check: {e}"), cx),
            });
        })
        .detach();
    }

    // ----- find and replace ---------------------------------------------------------------

    pub(super) fn find_replace_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.open.as_ref().map(|t| t.path.clone()) else { return };
        let query = new_input(window, cx, "", "Text to find (any case)");
        let with = new_input(window, cx, "", "Replace with");
        let names = Rc::new(RefCell::new(false));
        let found: Rc<RefCell<Option<J>>> = Rc::new(RefCell::new(None));
        let me = cx.entity().downgrade();
        let focus = focus_once(&query);
        window.open_dialog(cx, move |dialog, window, cx| {
            focus(window, cx);
            let theme = cx.theme().clone();
            let (q, w) = (query.read(cx).value().to_string(), with.read(cx).value().to_string());
            let run = {
                let (me, path, names, found) = (me.clone(), path.clone(), names.clone(), found.clone());
                let (q, w) = (q.clone(), w.clone());
                move |apply: bool, cx: &mut App| {
                    let (me2, path, found) = (me.clone(), path.clone(), found.clone());
                    let call = match me.upgrade() {
                        Some(s) => s.read(cx).backend.call("package.findReplace", json!({ "path": path, "query": q, "replacement": w, "names": *names.borrow(), "apply": apply })),
                        None => return,
                    };
                    cx.spawn(async move |cx| {
                        let r = call.await;
                        cx.update(|cx| {
                            let _ = me2.update(cx, |this, cx| match r {
                                Ok(r) => {
                                    if apply {
                                        let n = r.json["applied"].as_u64().unwrap_or(0);
                                        this.added(&path, None, format!("Replaced {n} values (unsaved)"), cx);
                                    }
                                    *found.borrow_mut() = Some(r.json);
                                    cx.notify();
                                }
                                Err(e) => this.toast(Tone::Error, format!("Find and replace: {e}"), cx),
                            });
                            cx.refresh_windows();
                        });
                    })
                    .detach();
                }
            };
            let run = Rc::new(run);
            let mut list = v_flex().gap_0p5();
            let mut count = 0;
            if let Some(f) = found.borrow().as_ref() {
                let matches = f["matches"].as_array().cloned().unwrap_or_default();
                count = matches.len();
                for m in matches.iter().take(200) {
                    list = list.child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .child(div().w(px(220.0)).overflow_hidden().text_ellipsis().whitespace_nowrap().child(format!("{} {}", m["object"].as_str().unwrap_or_default(), m["prop"].as_str().unwrap_or("(name)"))))
                            .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(theme.muted_foreground).child(format!("{} → {}", m["current"].as_str().unwrap_or_default(), m["replacement"].as_str().unwrap_or_default()))),
                    );
                }
            }
            let names_on = *names.borrow();
            let names2 = names.clone();
            let (r1, r2) = (run.clone(), run.clone());
            dialog
                .title("Find and replace")
                .w(px(720.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(field("Find", &query, cx))
                        .child(field("Replace with", &with, cx))
                        .child(Checkbox::new("names").label("Also rename objects whose name contains it").checked(names_on).on_click(move |v, _, _| *names2.borrow_mut() = *v))
                        .child(h_flex().gap_2().child(Button::new("preview").small().label("Preview").disabled(q.trim().is_empty()).on_click(move |_, _, cx| r1(false, cx))).child(div().text_xs().text_color(theme.muted_foreground).child(if found.borrow().is_some() { format!("{count} matches (strings, names, enums and references)") } else { "Preview lists what would change".into() })))
                        .child(div().id("matches").max_h(px(320.0)).overflow_y_scroll().child(list)),
                )
                .footer(footer("Replace all", false, count > 0, move |window, cx| {
                    r2(true, cx);
                    window.close_dialog(cx);
                }))
        });
    }

    // ----- batch import --------------------------------------------------------------------

    pub(super) fn batch_import(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let dirs = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Folder of <package>.<object>.<ext> files".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut picked))) = dirs.await else { return };
            let Some(dir) = picked.pop().map(|d| d.to_string_lossy().to_string()) else { return };
            let Ok(call) = this.update(cx, |this, cx| {
                this.set_status("Reading the folder…", cx);
                this.backend.call("batch.plan", json!({ "dir": dir }))
            }) else { return };
            let r = call.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(r) => this.pending_batch = Some((dir, r.json)),
                Err(e) => this.toast(Tone::Error, format!("Batch import: {e}"), cx),
            });
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Show the batch plan once it is in (needs a window).
    pub(super) fn show_batch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((dir, plan)) = self.pending_batch.take() else { return };
        let entries = plan["entries"].as_array().cloned().unwrap_or_default();
        let mut packages: Vec<String> = entries.iter().filter_map(|e| e["packagePath"].as_str().map(|s| s.to_string())).collect();
        packages.sort();
        packages.dedup();
        let chosen = Rc::new(RefCell::new(packages.clone()));
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let mut list = v_flex().gap_0p5();
            for e in entries.iter().take(300) {
                let ok = e["ok"].as_bool().unwrap_or(false);
                list = list.child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .child(div().w(px(240.0)).overflow_hidden().text_ellipsis().whitespace_nowrap().child(e["name"].as_str().unwrap_or_default().to_string()))
                        .child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(if ok { theme.muted_foreground } else { theme.danger }).child(e["message"].as_str().unwrap_or_default().to_string())),
                );
            }
            let mut pkgs = v_flex().gap_0p5();
            for p in &packages {
                let on = chosen.borrow().contains(p);
                let (c, p2) = (chosen.clone(), p.clone());
                pkgs = pkgs.child(Checkbox::new(SharedString::from(format!("bp-{p}"))).label(file_name(p)).checked(on).on_click(move |v, _, cx| {
                    let mut c = c.borrow_mut();
                    c.retain(|x| x != &p2);
                    if *v {
                        c.push(p2.clone());
                    }
                    cx.refresh_windows();
                }));
            }
            let (me, dir, chosen) = (me.clone(), dir.clone(), chosen.clone());
            dialog
                .title("Batch import")
                .w(px(760.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_xs().text_color(theme.muted_foreground).child(format!("{} files: {} can be imported, {} cannot. They become unsaved edits of the packages chosen below.", plan["entries"].as_array().map(|a| a.len()).unwrap_or(0), plan["ok"], plan["failed"])))
                        .when(packages.len() > 1, |d| d.child(pkgs))
                        .child(div().id("batch").max_h(px(360.0)).overflow_y_scroll().child(list)),
                )
                .footer(footer("Import", false, plan["ok"].as_u64().unwrap_or(0) > 0, move |window, cx| {
                    window.close_dialog(cx);
                    let (dir, pk) = (dir.clone(), chosen.borrow().clone());
                    let _ = me.update(cx, |this, cx| {
                        let call = this.backend.call("batch.run", json!({ "dir": dir, "packages": pk }));
                        this.set_status("Importing…", cx);
                        cx.spawn(async move |this, cx| {
                            let r = call.await;
                            let _ = this.update(cx, |this, cx| match r {
                                Ok(r) => {
                                    for p in &pk {
                                        this.dirty.insert(p.clone());
                                    }
                                    let open = this.open.as_ref().map(|t| t.path.clone());
                                    if let Some(o) = open.filter(|o| pk.contains(o)) {
                                        this.refresh_package(o, None, cx);
                                    }
                                    this.toast(Tone::Success, format!("Imported {} files ({} failed); save the packages to keep them", r.json["ok"], r.json["failed"]), cx);
                                }
                                Err(e) => this.toast(Tone::Error, format!("Batch import: {e}"), cx),
                            });
                        })
                        .detach();
                    });
                }))
        });
    }
}
