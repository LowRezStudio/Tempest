//! Signing and publishing, shared with the launcher through tempest-cli:
//! the signing key (`mod gen-key`, its fingerprint from `mod fingerprint`,
//! `mod sign-text` for the website's ownership check) and the mod
//! repository's API key (`mod login`, kept in `repo.json`); **Publish**
//! uploads the built, signed package with its listing (`mod publish
//! --json`), asking for an API key in place when the repository refuses.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{div, prelude::*, px, AnyElement, ClipboardItem, Context, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, Textarea};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};

use super::cooker::{app_data, explain, run_cli_split, signing_key, Cooker};
use super::modmeta::{self, Published};
use super::{file_name, Tone};

#[derive(Clone, Debug, Default)]
pub struct Account {
    pub has_key: bool,
    /// `user id <fingerprint>` as `mod fingerprint` prints it.
    pub key_id: Option<String>,
    pub repo_url: String,
    /// The API key, masked; `None` when not logged in.
    pub api_key: Option<String>,
}

fn read_account() -> Account {
    let key = signing_key();
    let public = key.with_extension("pub");
    let key_id = key
        .exists()
        .then(|| run_cli_split(&["fingerprint".into(), if public.exists() { public } else { key.clone() }.to_string_lossy().to_string()]).ok())
        .flatten()
        .filter(|r| r.0 == 0)
        .map(|r| r.1.trim().to_string())
        .filter(|s| !s.is_empty());
    let config: serde_json::Value = std::fs::read_to_string(app_data().join("com.lowrezstudio.tempest").join("repo.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let api = config["apiKey"].as_str().map(|k| if k.len() <= 12 { format!("{}…", &k[..k.len().min(4)]) } else { format!("{}…{}", &k[..8], &k[k.len() - 4..]) });
    let url = std::env::var("TEMPEST_REPO_URL").ok().or_else(|| config["url"].as_str().map(str::to_string)).unwrap_or_else(|| "https://mods.lowrezstudio.com".into());
    Account { has_key: key.exists(), key_id, repo_url: url.trim_end_matches('/').to_string(), api_key: api }
}

/// The Publish dialog's state, shared by its builder and the calls it starts.
#[derive(Default)]
struct PublishState {
    busy: bool,
    result: Option<Published>,
    error: Option<String>,
    need_login: bool,
    login_error: Option<String>,
}

impl Cooker {
    pub(super) fn load_account(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let a = cx.background_executor().spawn(async { read_account() }).await;
            let _ = this.update(cx, |this, cx| {
                this.account = Some(a);
                cx.notify();
            });
        })
        .detach();
    }

    /// Run `tempest-cli mod <args>` off the UI thread, then `done` with (code, stdout, stderr).
    fn cli(&mut self, args: Vec<String>, busy: &str, cx: &mut Context<Self>, done: impl FnOnce(&mut Cooker, Result<(i32, String, String), String>, &mut Context<Cooker>) + 'static) {
        self.busy = Some(busy.to_string());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = cx.background_executor().spawn(async move { run_cli_split(&args) }).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                done(this, r, cx);
                this.load_account(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn create_key(&mut self, id: String, cx: &mut Context<Self>) {
        let id = if id.trim().is_empty() { "tempest-studio".to_string() } else { id.trim().to_string() };
        self.cli(vec!["gen-key".into(), "--id".into(), id], "Creating a signing key…", cx, |this, r, cx| match r {
            Ok((0, _, _)) => this.toast(Tone::Success, "Signing key created: your mods are signed with it from now on", cx),
            // A key already on this machine is used as it is.
            Ok((_, out, err)) if format!("{out}{err}").to_ascii_lowercase().contains("already exists") => this.toast(Tone::Info, "A signing key already exists on this machine", cx),
            Ok((_, out, err)) => {
                let raw = if err.trim().is_empty() { out.trim() } else { err.trim() };
                this.toast(Tone::Error, format!("Could not create the key: {}", super::ui::explain_error(raw)), cx);
            }
            Err(e) => this.toast(Tone::Error, format!("Could not create the key: {}", super::ui::explain_error(&e)), cx),
        });
    }

    fn forget_api_key(&mut self, cx: &mut Context<Self>) {
        self.cli(vec!["login".into()], "Forgetting the API key…", cx, |this, r, cx| match r {
            Ok((0, _, _)) => this.toast(Tone::Info, "The API key is forgotten", cx),
            Ok((code, out, err)) => {
                let raw = if !err.trim().is_empty() { err.trim().to_string() } else if !out.trim().is_empty() { out.trim().to_string() } else { format!("exit {code}") };
                this.toast(Tone::Error, format!("Could not forget the API key: {}", super::ui::explain_error(&raw)), cx);
            }
            Err(e) => this.toast(Tone::Error, format!("Could not forget the API key: {}", super::ui::explain_error(&e)), cx),
        });
    }

    /// `mod login <key>`; `then` runs once the repository accepted it.
    fn login(&mut self, key: String, cx: &mut Context<Self>, then: impl FnOnce(&mut Cooker, Option<String>, &mut Context<Cooker>) + 'static) {
        self.cli(vec!["login".into(), key.trim().to_string()], "Logging in…", cx, move |this, r, cx| {
            let why = match r {
                Ok((0, _, _)) => None,
                Ok((code, out, err)) => {
                    let raw = if !err.trim().is_empty() { err.trim().to_string() } else if !out.trim().is_empty() { out.trim().to_string() } else { format!("exit {code}") };
                    Some(super::ui::explain_error(&raw))
                }
                Err(e) => Some(super::ui::explain_error(&e)),
            };
            then(this, why, cx);
        });
    }

    /// The Signing and publishing section of the Mod view.
    pub(super) fn render_account(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let a = self.account.clone().unwrap_or_default();
        let busy_reason = self.busy.clone();
        let busy = busy_reason.is_some();
        let api_empty = self.meta.api_key.read(cx).value().trim().is_empty();
        let muted = theme.muted_foreground;
        let key_line = match (&a.key_id, a.has_key) {
            (Some(id), _) => format!("Signing key: {id}"),
            (None, true) => "Signing key: on this machine".to_string(),
            (None, false) => "No signing key yet: mods with code (Lua, DLLs) need one".to_string(),
        };
        let author = self.meta.text("author", cx);
        let url = a.repo_url.clone();
        let (u1, u2) = (url.clone(), url.clone());
        let login_tip = if let Some(b) = &busy_reason {
            b.clone()
        } else if api_empty {
            "Paste an API key from your account settings first".to_string()
        } else if a.api_key.is_some() {
            "Replace the saved API key".to_string()
        } else {
            "Save this API key for publishing mods".to_string()
        };
        let logout_tip = busy_reason.clone().unwrap_or_else(|| "Remove the saved API key from this computer".to_string());
        v_flex()
            .gap_2()
            .child(div().text_sm().child(key_line))
            .child(
                h_flex()
                    .gap_2()
                    .when(!a.has_key, |d| {
                        let tip = busy_reason.clone().unwrap_or_else(|| "Create one signing key for your mods, shared with Tempest Launcher".to_string());
                        d.child(Button::new("acc-gen-key").xsmall().label("Create my signing key").disabled(busy).tooltip(tip).on_click(cx.listener(move |this, _, _, cx| this.create_key(author.clone(), cx))))
                    })
                    .when(a.has_key, |d| {
                        let tip = busy_reason.clone().unwrap_or_else(|| "Sign the text the website shows, to link this key to your account".to_string());
                        d.child(Button::new("acc-verify").xsmall().label("Prove it is my key…").disabled(busy).tooltip(tip).on_click(cx.listener(|this, _, window, cx| this.sign_text_dialog(window, cx))))
                            .child(Button::new("acc-key-page").xsmall().ghost().label("Keys on the website").on_click(move |_, _, cx| cx.open_url(&format!("{u1}/settings#signing-keys"))))
                    }),
            )
            .child(div().text_sm().child(match &a.api_key {
                Some(k) => format!("Mod repository: logged in ({k}) at {}", a.repo_url.trim_start_matches("https://")),
                None => format!("Mod repository: not logged in ({})", a.repo_url.trim_start_matches("https://")),
            }))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(Input::new(&self.meta.api_key).small()))
                    .child(Button::new("acc-login").xsmall().label(if a.api_key.is_some() { "Replace" } else { "Log in" }).disabled(busy || api_empty).tooltip(login_tip).on_click(cx.listener(|this, _, _, cx| {
                        let key = this.meta.api_key.read(cx).value().to_string();
                        if key.trim().is_empty() {
                            return;
                        }
                        this.login(key, cx, |this, why, cx| match why {
                            None => this.toast(Tone::Success, "Logged in to the mod repository", cx),
                            Some(e) => this.toast(Tone::Error, format!("The repository refused the key: {e}"), cx),
                        });
                    })))
                    .when(a.api_key.is_some(), |d| d.child(Button::new("acc-logout").xsmall().ghost().label("Forget").disabled(busy).tooltip(logout_tip).on_click(cx.listener(|this, _, _, cx| this.forget_api_key(cx))))),
            )
            .child(h_flex().gap_1().text_xs().text_color(muted).child("API keys are made in your account settings on the website:").child(Button::new("acc-api-page").xsmall().ghost().label("open").on_click(move |_, _, cx| cx.open_url(&format!("{u2}/settings#api-keys")))))
            .into_any_element()
    }

    /// Sign a text with the key (`mod sign-text`): paste the website's challenge, copy the signature back.
    fn sign_text_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx.new(|cx| gpui_component::input::TextareaState::new(window, cx).placeholder("Paste the text the website asks you to sign").auto_grow(3, 8));
        cx.subscribe(&text, |_, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.refresh_windows();
            }
        })
        .detach();
        let out: Rc<RefCell<Option<Result<String, String>>>> = Rc::default();
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let (out2, text2, me) = (out.clone(), text.clone(), me.clone());
            let me_copy = me.clone();
            let result = out.borrow().clone();
            let has_sig = matches!(&result, Some(Ok(_)));
            let signing = me.upgrade().and_then(|c| c.read(cx).busy.clone());
            let empty = text.read(cx).value().trim().is_empty();
            let sign_tip = if let Some(b) = &signing {
                b.clone()
            } else if empty {
                "Paste the challenge text from the website first".to_string()
            } else {
                "Sign the text with your local key".to_string()
            };
            dialog
                .title("Prove the signing key is yours")
                .w(px(620.0))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_xs().text_color(theme.muted_foreground).child("The website shows a text to sign when you add your key. Paste it here; the signature it gives back proves the key is on your machine. The key itself never leaves it."))
                        .child(Textarea::new(&text))
                        .when_some(result, |d, r| match r {
                            Ok(sig) => {
                                let copy = sig.clone();
                                d.child(div().id("sig").max_h(px(180.0)).overflow_y_scroll().p_2().rounded_md().bg(theme.secondary).text_xs().font_family(crate::studio::ui::mono()).child(sig)).child(h_flex().child(Button::new("sig-copy").small().primary().label("Copy the signature").on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                    let _ = me_copy.update(cx, |this, cx| this.toast(Tone::Success, "Signature copied to clipboard", cx));
                                })))
                            }
                            Err(e) => d.child(div().text_xs().text_color(theme.danger).whitespace_normal().child(e)),
                        }),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(Button::new("sig-close").label("Close").on_click(|_, window, cx| window.close_dialog(cx)))
                        .child(
                            Button::new("sig-sign")
                                .when(!has_sig, |b| b.primary())
                                .label(if signing.is_some() { "Signing…" } else if has_sig { "Sign again" } else { "Sign" })
                                .disabled(signing.is_some() || empty)
                                .tooltip(sign_tip)
                                .on_click(move |_, _, cx| {
                                    let t = text2.read(cx).value().to_string();
                                    if t.trim().is_empty() {
                                        return;
                                    }
                                    let out = out2.clone();
                                    let _ = me.update(cx, move |this, cx| {
                                        this.cli(vec!["sign-text".into(), t], "Signing…", cx, move |_, r, cx| {
                                            *out.borrow_mut() = Some(match r {
                                                Ok((0, sig, _)) => Ok(sig.trim().to_string()),
                                                Ok((_, o, e)) => Err(super::ui::explain_error(if e.trim().is_empty() { o.trim() } else { e.trim() })),
                                                Err(e) => Err(super::ui::explain_error(&e)),
                                            });
                                            cx.refresh_windows();
                                        });
                                    });
                                }),
                        ),
                )
        });
    }

    /// The Publish dialog: listing, game patches, changelog, then the repository's answer.
    pub(super) fn publish_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((pkg, signed)) = self.built.clone() else { return };
        let state: Rc<RefCell<PublishState>> = Rc::default();
        let me = cx.entity().downgrade();
        let name = self.meta.text("name", cx);
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let Some(this) = me.upgrade() else { return dialog };
            let c = this.read(cx);
            let (pkg, signed) = c.built.clone().unwrap_or_else(|| (pkg.clone(), signed));
            let repack_release = c.built_fast && !c.meta.fast;
            let busy_label = c.busy.clone().unwrap_or_else(|| "Publishing…".to_string());
            let m = &c.meta;
            let repo = c.account.as_ref().map(|a| a.repo_url.clone()).unwrap_or_else(|| "https://mods.lowrezstudio.com".into());
            let summary = m.text("summary", cx);
            let compat = !m.compat.covered().is_empty();
            let summary_ok = !summary.is_empty() && summary.chars().count() <= modmeta::MAX_SUMMARY;
            let api_empty = m.api_key.read(cx).value().trim().is_empty();
            let st = state.borrow();
            let need_login = st.need_login;
            let stale = c.built_as.as_ref().is_some_and(|(i, v)| *i != c.mod_id(cx) || *v != c.version(cx));
            let listing = [modmeta::tags_problem(&m.text("tags", cx)), modmeta::repository_problem(&m.text("repository", cx))];
            let listing_ok = listing.iter().all(Option::is_none);
            let can = compat && summary_ok && signed && listing_ok && !stale && !st.busy && !need_login;
            let mut why: Vec<String> = Vec::new();
            if stale {
                why.push("The id or version changed since this build: build again so the upload carries them.".into());
            }
            if !signed {
                why.push("The package is not signed: create your signing key in Publish ▸ Signing and account, then build again.".into());
            }
            if !summary_ok {
                why.push("Write a summary (140 characters at most) in Publish ▸ Listing.".into());
            }
            if !compat {
                why.push("Pick the game patches it works with in Publish.".into());
            }
            why.extend(listing.into_iter().flatten().map(|w| format!("{w} (Publish ▸ Listing).")));
            let first_why = why.first().cloned();
            let works = m.compat.describe();
            let changelog = m.changelog.read(cx).value().trim().to_string();
            let fact = |label: &'static str, value: String| h_flex().gap_2().items_start().child(div().flex_none().w(px(110.0)).text_xs().text_color(theme.muted_foreground).child(label)).child(div().flex_1().min_w_0().text_xs().whitespace_normal().child(value));
            if let Some(r) = st.result.clone().filter(|r| r.state == "done") {
                let id = r.mod_id.clone().unwrap_or_else(|| c.mod_id(cx));
                let page = format!("{repo}/mods/{id}");
                return dialog
                    .title(format!("Publish {name}"))
                    .w(px(560.0))
                    .child(
                        v_flex()
                            .gap_2()
                            .items_center()
                            .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(format!("{id} {} is published", r.version.clone().unwrap_or_default())))
                            .children(r.warnings.iter().map(|w| div().text_xs().text_color(theme.warning).child(w.clone())))
                            .child(Button::new("pub-view").primary().label("View it online").on_click(move |_, _, cx| cx.open_url(&page))),
                    );
            }
            let mut body = v_flex()
                .gap_2()
                .child(div().text_xs().text_color(theme.muted_foreground).child(format!("Uploads {} to {}. A new mod is reviewed before it is listed.", file_name(&pkg), repo.trim_start_matches("https://"))))
                .child(fact("Summary", if summary.is_empty() { "none".into() } else { summary.clone() }))
                .child(fact("Works with", if works.is_empty() { "no patch named".into() } else { works }))
                .child(fact("What changed", if changelog.is_empty() { "nothing written".into() } else { changelog }))
                .when(repack_release, |d| {
                    d.child(div().text_xs().text_color(theme.muted_foreground).whitespace_normal().child("This build used fast compression for local testing; Publish will repack it to the smallest download size before uploading."))
                })
                .child(div().text_xs().text_color(theme.muted_foreground).child("Change these in the Publish tab of the Mod view."))
                .children(why.into_iter().map(|w| div().text_xs().text_color(theme.warning).child(w)));
            if let Some(r) = &st.result {
                let text = if r.state == "queued" || r.state == "pending" { "Uploaded: it is waiting for review".to_string() } else { format!("The repository answered: {}", r.state) };
                body = body.child(div().text_sm().child(text)).children(r.errors.iter().map(|e| div().text_xs().text_color(theme.danger).whitespace_normal().child(e.clone()))).children(r.warnings.iter().map(|w| div().text_xs().text_color(theme.warning).whitespace_normal().child(w.clone())));
            }
            if let Some(e) = &st.error {
                body = body.child(div().text_xs().text_color(theme.danger).whitespace_normal().child(e.clone()));
            }
            if need_login {
                let (me2, state2) = (me.clone(), state.clone());
                let login_btn_tip = if st.busy {
                    busy_label.clone()
                } else if api_empty {
                    "Paste an API key from your account settings first".to_string()
                } else {
                    "Save your API key and upload the mod".to_string()
                };
                body = body.child(
                    v_flex()
                        .gap_1()
                        .p_2()
                        .rounded_md()
                        .bg(theme.secondary)
                        .child(div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).child("Log in to the mod repository"))
                        .child(div().text_xs().text_color(theme.muted_foreground).child("Paste an API key from your account settings on the website; it is kept for the launcher too."))
                        .child(h_flex().gap_2().child(div().flex_1().min_w_0().child(Input::new(&m.api_key).small())).child(Button::new("pub-login").small().primary().label(if st.busy { "Logging in…" } else { "Log in and publish" }).disabled(st.busy || api_empty).tooltip(login_btn_tip).on_click(move |_, _, cx| {
                            let state = state2.clone();
                            let _ = me2.update(cx, |this, cx| {
                                let key = this.meta.api_key.read(cx).value().to_string();
                                if key.trim().is_empty() {
                                    return;
                                }
                                state.borrow_mut().busy = true;
                                this.login(key, cx, move |this, why, cx| match why {
                                    None => {
                                        state.borrow_mut().need_login = false;
                                        this.publish(state, cx);
                                    }
                                    Some(e) => {
                                        let mut s = state.borrow_mut();
                                        s.busy = false;
                                        s.login_error = Some(e);
                                        cx.refresh_windows();
                                    }
                                });
                            });
                        })))
                        .when_some(st.login_error.clone(), |d, e| d.child(div().text_xs().text_color(theme.danger).whitespace_normal().child(e))),
                );
            }
            let (me3, state3) = (me.clone(), state.clone());
            let busy = st.busy;
            drop(st);
            let pub_tip = if need_login {
                "Enter your API key above to log in and publish".to_string()
            } else if busy {
                busy_label.clone()
            } else if let Some(w) = first_why {
                w.to_string()
            } else {
                "Upload the signed mod package to the repository".to_string()
            };
            dialog.title(format!("Publish {name}")).w(px(620.0)).child(body).footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("pub-cancel").label("Close").on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(
                        Button::new("pub-go")
                            .when(!need_login, |b| b.primary())
                            .label(if busy { busy_label } else { "Publish".to_string() })
                            .disabled(!can)
                            .tooltip(pub_tip)
                            .on_click(move |_, _, cx| {
                                let state = state3.clone();
                                let _ = me3.update(cx, |this, cx| this.publish(state, cx));
                            }),
                    ),
            )
        });
    }

    fn publish(&mut self, state: Rc<RefCell<PublishState>>, cx: &mut Context<Self>) {
        let Some((pkg, _)) = self.built.clone() else { return };
        if self.built_fast && !self.meta.fast {
            {
                let mut s = state.borrow_mut();
                s.busy = true;
                s.error = None;
                s.result = None;
            }
            cx.refresh_windows();
            self.build_release_into(pkg, cx, move |this, r, cx| match r {
                Ok(()) => this.publish_built(state, cx),
                Err(e) => {
                    let mut s = state.borrow_mut();
                    s.busy = false;
                    s.error = Some(explain(&e));
                    cx.refresh_windows();
                }
            });
            return;
        }
        self.publish_built(state, cx);
    }

    fn publish_built(&mut self, state: Rc<RefCell<PublishState>>, cx: &mut Context<Self>) {
        let Some((pkg, _)) = self.built.clone() else { return };
        let m = &self.meta;
        let mut args = vec!["publish".to_string(), pkg];
        for p in &m.compat.patches {
            args.extend(["--patch".into(), p.to_string()]);
        }
        let changelog = m.changelog.read(cx).value().trim().to_string();
        for (flag, v) in [("--from", m.compat.from.clone().unwrap_or_default()), ("--to", m.compat.to.clone().unwrap_or_default()), ("--summary", m.text("summary", cx)), ("--changelog", changelog), ("--tags", m.text("tags", cx)), ("--repository", m.text("repository", cx))] {
            if !v.is_empty() {
                args.extend([flag.to_string(), v]);
            }
        }
        args.push("--json".into());
        {
            let mut s = state.borrow_mut();
            s.busy = true;
            s.error = None;
            s.result = None;
        }
        cx.refresh_windows();
        self.cli(args, "Publishing…", cx, move |this, r, cx| {
            let mut s = state.borrow_mut();
            s.busy = false;
            match r {
                Ok((code, out, err)) => match modmeta::parse_publish(&out) {
                    Some(mut p) => {
                        if p.state == "done" {
                            this.toast(Tone::Success, "Mod published", cx);
                        }
                        p.errors = p.errors.into_iter().map(|e| super::ui::explain_error(&e)).collect();
                        s.result = Some(p);
                    }
                    None => {
                        let msg = if !err.trim().is_empty() { err.trim().to_string() } else if !out.trim().is_empty() { out.trim().to_string() } else { format!("exit {code}") };
                        if modmeta::is_auth_error(&msg) {
                            s.need_login = true;
                        } else if code == 0 {
                            s.result = Some(Published { state: "done".into(), ..Default::default() });
                        } else {
                            s.error = Some(super::ui::explain_error(msg.trim_start_matches("Publish failed:").trim()));
                        }
                    }
                },
                Err(e) => s.error = Some(super::ui::explain_error(&e)),
            }
            cx.refresh_windows();
        });
    }

    /// Save a copy of the built package elsewhere.
    pub(super) fn save_built_copy(&mut self, cx: &mut Context<Self>) {
        let Some((pkg, _)) = self.built.clone() else { return };
        let docs = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        let target = cx.prompt_for_new_path(&docs, Some(&file_name(&pkg)));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(out))) = target.await else { return };
            let r = std::fs::copy(&pkg, &out);
            let _ = this.update(cx, |this, cx| match r {
                Ok(_) => this.toast(Tone::Success, format!("Saved {}", out.display()), cx),
                Err(e) => this.toast(Tone::Error, format!("Could not save the copy: {}", super::ui::explain_error(&e.to_string())), cx),
            });
        })
        .detach();
    }
}

