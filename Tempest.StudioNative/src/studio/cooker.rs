//! The Mod view: a mod is the list of game files your changes touch. Each
//! one is listed with where its edits are (unsaved, saved in place, an
//! exported copy) and how it ships (a delta of only what changed, or the
//! whole file); its changes can be previewed. Details name the mod; Build
//! writes the mod folder (`mod.cook`), packs and signs it with tempest-cli
//! like the launcher, and Install puts it into a game folder.

use std::path::{Path, PathBuf};

use gpui::{div, prelude::*, px, AnyElement, App, Context, Entity, PathPromptOptions, SharedString, Subscription, WeakEntity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, Selectable};
use serde_json::{json, Value as J};

use super::{file_name, modmeta, state_file, Studio, Tone};
use crate::backend::Backend;

struct Target {
    target: String,
    rel: String,
    kind: String,
    script: bool,
    /// The sources (id, path, origin) and which one ships.
    sources: Vec<(String, String, String)>,
    chosen: usize,
    whole: bool,
    included: bool,
    base: String,
    preview: Option<J>,
    /// The `.dat` document id (assembly files), for Discard.
    doc: Option<String>,
    /// When the in-place save of this file was written (`mod.changes`' `modified`).
    saved_modified: Option<u64>,
}

#[derive(Clone, Copy, PartialEq)]
enum Line {
    Info,
    Ok,
    Error,
}

pub struct Cooker {
    backend: Backend,
    studio: WeakEntity<Studio>,
    dir: String,
    targets: Vec<Target>,
    loading: bool,
    pub(super) meta: Meta,
    pub(super) busy: Option<String>,
    log: Vec<(Line, String)>,
    /// The package built last and whether it was signed.
    pub(super) built: Option<(String, bool)>,
    /// The open mod project: its details and folders.
    project: Option<super::project::Project>,
    /// Files added to the mod as they are: (source, path inside the mod).
    extras: Vec<(String, String)>,
    /// Game files saved in place taken off the list, by their modified time then.
    dismissed: std::collections::BTreeMap<String, u64>,
    /// A file added that names no game file, waiting for the user's answer.
    unmatched: Option<String>,
    /// Copies just added: they become the shipped source of their game file.
    choose: Vec<String>,
    pub(super) account: Option<super::account::Account>,
    /// The edit count the list was read at: the list follows edits made anywhere.
    seen: usize,
    /// The tab shown: what goes in, the details, publishing.
    tab: ModTab,
    /// Build & install: install into this game once the build is done.
    install_after: bool,
    _subs: Vec<Subscription>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModTab {
    Changes,
    Details,
    Publish,
}

/// The details the user types: the mod, its listing and the publish answers.
pub struct Meta {
    pub name: Entity<InputState>,
    pub id: Entity<InputState>,
    pub version: Entity<InputState>,
    pub author: Entity<InputState>,
    pub link: Entity<InputState>,
    pub summary: Entity<InputState>,
    pub tags: Entity<InputState>,
    pub repository: Entity<InputState>,
    pub deps: Entity<TextareaState>,
    pub readme: Entity<TextareaState>,
    pub patches: Entity<InputState>,
    pub patch_from: Entity<InputState>,
    pub patch_to: Entity<InputState>,
    pub changelog: Entity<TextareaState>,
    pub api_key: Entity<InputState>,
    pub icon: Option<String>,
    /// zstd instead of LZMA: builds several times faster, a mod about 30 % bigger.
    pub fast: bool,
}

/// The single-line fields: (key in the saved JSON, placeholder).
const FIELDS: &[(&str, &str)] = &[
    ("name", "My mod"),
    ("id", "made from the name"),
    ("version", "1.0.0"),
    ("author", "Your name"),
    ("link", "a link to you (optional)"),
    ("summary", "One line for the mod list (140 characters at most)"),
    ("tags", "comma separated, e.g. skin, maeve"),
    ("repository", "https://github.com/you/your-mod (optional)"),
    ("patches", "game patches it works with, e.g. 8.1"),
    ("patchFrom", "or from patch…"),
    ("patchTo", "…to patch (optional)"),
];

impl Meta {
    fn input(&self, key: &str) -> &Entity<InputState> {
        match key {
            "name" => &self.name,
            "id" => &self.id,
            "version" => &self.version,
            "author" => &self.author,
            "link" => &self.link,
            "summary" => &self.summary,
            "tags" => &self.tags,
            "repository" => &self.repository,
            "patches" => &self.patches,
            "patchFrom" => &self.patch_from,
            _ => &self.patch_to,
        }
    }

    pub fn text(&self, key: &str, cx: &App) -> String {
        self.input(key).read(cx).value().trim().to_string()
    }

    pub fn deps(&self, cx: &App) -> Vec<String> {
        self.deps.read(cx).value().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
    }
}

/// `My Cool Mod` → `my-cool-mod`.
fn id_of(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}


/// tempest-cli: $TEMPEST_CLI, next to this program, or the repository's build.
fn cli_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("TEMPEST_CLI").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    let exe = if cfg!(windows) { "tempest-cli.exe" } else { "tempest-cli" };
    if let Some(p) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(exe))).filter(|p| p.exists()) {
        return Some(p);
    }
    let dev = if cfg!(windows) { "Tempest.CLI.exe" } else { "Tempest.CLI" };
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("Tempest.CLI").join("bin");
    ["Release", "Debug"].iter().map(|c| repo.join(c).join("net10.0").join(dev)).find(|p| p.exists())
}

pub(super) fn app_data() -> PathBuf {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support")).unwrap_or_default()
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default()
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))).unwrap_or_default()
    }
}

pub(super) fn signing_key() -> PathBuf {
    std::env::var_os("TEMPEST_SIGNING_KEY").map(PathBuf::from).unwrap_or_else(|| app_data().join("com.lowrezstudio.tempest").join("signing").join("tempest.key"))
}

/// `tempest-cli mod <args>`: exit code, stdout and stderr.
pub(super) fn run_cli_split(args: &[String]) -> Result<(i32, String, String), String> {
    let cli = cli_path().ok_or("tempest-cli was not found (build Tempest.CLI, or set TEMPEST_CLI)")?;
    let out = std::process::Command::new(cli).arg("mod").args(args).output().map_err(|e| e.to_string())?;
    Ok((out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string()))
}

/// A game file as a player names it: `ChaosGame/CookedPCConsole/assembly.dat` → `Game data`.
fn file_in_words(rel: &str) -> String {
    let name = rel.rsplit(['/', '\\']).next().unwrap_or(rel);
    let lower = name.to_ascii_lowercase();
    if lower == "assembly.dat" {
        "Game data".into()
    } else if lower.starts_with("lang_") && lower.ends_with(".dat") {
        format!("Texts ({})", &name[5..name.len() - 4])
    } else if lower.ends_with(".pck") {
        format!("Sounds: {}", &name[..name.len() - 4])
    } else {
        name.trim_end_matches(".upk").to_string()
    }
}

/// The name `tempest mod list --json` records the mod `id` under.
fn installed_name(list: &J, id: &str) -> Option<String> {
    let field = |m: &J, k: &str| m.get(k).or_else(|| m.get(format!("{}{}", k[..1].to_uppercase(), &k[1..]))).and_then(|v| v.as_str()).map(str::to_string);
    let mods = list.as_array().or_else(|| list.get("mods").or_else(|| list.get("Mods")).and_then(|m| m.as_array()))?;
    mods.iter().find(|m| field(m, "id").is_some_and(|i| i.eq_ignore_ascii_case(id))).and_then(|m| field(m, "name"))
}

/// `tempest-cli mod <args>`: exit code and the output (stdout then stderr).
fn run_cli(args: &[String]) -> Result<(i32, String), String> {
    run_cli_split(args).map(|(code, out, err)| (code, out + &err))
}

impl Cooker {
    pub fn new(backend: Backend, dir: String, project: Option<super::project::Project>, studio: WeakEntity<Studio>, window: &mut Window, cx: &mut Context<Self>) -> Cooker {
        // A project keeps the details in its file; otherwise they are kept per game folder.
        let saved: J = match project.as_ref().map(|p| p.cooker.clone()).filter(|c| c.is_object()) {
            Some(c) => c,
            None => meta_file(&dir).and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(J::Null),
        };
        let mut saved = saved;
        if let Some(p) = &project {
            for (k, v) in [("name", &p.name), ("id", &p.id), ("version", &p.version), ("author", &p.author)] {
                if saved[k].as_str().is_none_or(str::is_empty) {
                    saved[k] = json!(v);
                }
            }
        }
        let mut inputs: Vec<Entity<InputState>> = FIELDS
            .iter()
            .map(|(k, ph)| {
                let v = saved[*k].as_str().unwrap_or_default().to_string();
                cx.new(|cx| {
                    let mut s = InputState::new(window, cx).placeholder(ph.to_string());
                    s.set_value(v, window, cx);
                    s
                })
            })
            .collect();
        let area = |key: &str, ph: &str, rows: usize, window: &mut Window, cx: &mut Context<Self>| {
            let (v, ph) = (saved[key].as_str().unwrap_or_default().to_string(), ph.to_string());
            cx.new(|cx| {
                let mut s = TextareaState::new(window, cx).placeholder(ph).auto_grow(rows, 12);
                s.set_value(v, window, cx);
                s
            })
        };
        let deps = area("dependencies", "one per line, e.g. other-mod >=1.2.0", 2, window, cx);
        let readme = area("readme", "What the mod does, in Markdown (its readme)", 3, window, cx);
        let changelog = area("changelog", "What changed in this version (optional)", 2, window, cx);
        let api_key = cx.new(|cx| InputState::new(window, cx).placeholder("API key from your account settings").masked(true));
        let mut take = || inputs.remove(0);
        let meta = Meta {
            name: take(),
            id: take(),
            version: take(),
            author: take(),
            link: take(),
            summary: take(),
            tags: take(),
            repository: take(),
            patches: take(),
            patch_from: take(),
            patch_to: take(),
            deps,
            readme,
            changelog,
            api_key,
            icon: saved["icon"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
            fast: saved["compression"].as_str() == Some("fast"),
        };
        let on_change = |this: &mut Cooker, ev: &InputEvent, cx: &mut Context<Cooker>| {
            if matches!(ev, InputEvent::Change) {
                this.save_meta(cx);
                // The Publish dialog's checks read these fields too.
                cx.refresh_windows();
            }
        };
        let mut subs: Vec<Subscription> = FIELDS.iter().map(|(k, _)| cx.subscribe(meta.input(k), move |this: &mut Cooker, _, ev: &InputEvent, cx| on_change(this, ev, cx))).collect();
        for t in [&meta.deps, &meta.readme, &meta.changelog] {
            subs.push(cx.subscribe(t, move |this: &mut Cooker, _, ev: &InputEvent, cx| on_change(this, ev, cx)));
        }
        let extras = saved["extras"].as_array().into_iter().flatten().filter_map(|x| Some((x["src"].as_str()?.to_string(), x["dest"].as_str()?.to_string()))).collect();
        let dismissed = saved["dismissed"].as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.clone(), v.as_u64()?))).collect();
        let mut c = Cooker { backend, studio, dir, targets: Vec::new(), loading: true, meta, busy: None, log: Vec::new(), built: None, project, extras, dismissed, unmatched: None, choose: Vec::new(), account: None, seen: crate::backend::EDITS.load(std::sync::atomic::Ordering::SeqCst), tab: ModTab::Changes, install_after: false, _subs: subs };
        c.refresh(cx);
        c.load_account(cx);
        c
    }

    pub(super) fn toast(&self, tone: Tone, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        let _ = self.studio.update(cx, |s, cx| s.toast(tone, text, cx));
    }

    fn say(&mut self, line: Line, text: impl Into<String>, cx: &mut Context<Self>) {
        self.log.push((line, text.into()));
        cx.notify();
    }

    fn meta_json(&self, cx: &App) -> J {
        let mut v = json!({
            "dependencies": self.meta.deps.read(cx).value().to_string(),
            "readme": self.meta.readme.read(cx).value().to_string(),
            "changelog": self.meta.changelog.read(cx).value().to_string(),
            "icon": self.meta.icon,
            "compression": if self.meta.fast { "fast" } else { "small" },
            "extras": self.extras.iter().map(|(s, d)| json!({ "src": s, "dest": d })).collect::<Vec<_>>(),
            "dismissed": self.dismissed,
        });
        for (k, _) in FIELDS {
            v[*k] = json!(self.meta.input(k).read(cx).value().to_string());
        }
        v
    }

    pub(super) fn save_meta(&mut self, cx: &mut Context<Self>) {
        let v = self.meta_json(cx);
        if self.project.is_some() {
            let (name, id, author, version) = (self.meta.text("name", cx), self.mod_id(cx), self.meta.text("author", cx), self.meta.text("version", cx));
            if let Some(p) = &mut self.project {
                (p.name, p.id, p.author, p.version, p.cooker) = (name.clone(), id.clone(), author.clone(), version.clone(), v.clone());
            }
            let _ = self.studio.update(cx, |s, cx| s.set_project_details(name, id, author, version, v, cx));
            return;
        }
        if let Some(f) = meta_file(&self.dir) {
            let _ = std::fs::create_dir_all(f.parent().unwrap());
            let _ = std::fs::write(f, v.to_string());
        }
    }

    pub(super) fn mod_id(&self, cx: &App) -> String {
        let typed = self.meta.text("id", cx);
        if typed.is_empty() { id_of(&self.meta.text("name", cx)) } else { typed }
    }

    pub(super) fn version(&self, cx: &App) -> String {
        let v = self.meta.text("version", cx);
        if v.is_empty() { "1.0.0".into() } else { v }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let call = self.backend.call("mod.changes", json!({}));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match r {
                    Ok(r) => {
                        // A project's mod lists only what was edited in it (and what is unsaved now).
                        let mut files = r.json["files"].clone();
                        let project = this.studio.upgrade().and_then(|s| s.read(cx).project.as_ref().map(|p| p.files.clone()));
                        if let (Some(names), Some(list)) = (project, files.as_array_mut()) {
                            list.retain(|f| f["origin"].as_str() == Some("unsaved") || names.contains(&file_name(f["target"].as_str().unwrap_or_default()).to_ascii_lowercase()));
                        }
                        this.set_sources(&files);
                        // What changed in each file shows without asking (the first few: a big package takes a moment).
                        for k in 0..this.targets.len().min(8) {
                            if this.targets[k].included {
                                this.preview(k, cx);
                            }
                        }
                    }
                    Err(e) => this.toast(Tone::Error, format!("Could not list the changes: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_sources(&mut self, files: &J) {
        let old: Vec<(String, bool, bool)> = self.targets.iter().map(|t| (t.target.clone(), t.included, t.whole)).collect();
        let mut targets: Vec<Target> = Vec::new();
        for f in files.as_array().into_iter().flatten() {
            let target = f["target"].as_str().unwrap_or_default().to_string();
            let src = (f["id"].as_str().unwrap_or_default().to_string(), f["source"].as_str().unwrap_or_default().to_string(), f["origin"].as_str().unwrap_or_default().to_string());
            let modified = (src.2 == "saved").then(|| f["modified"].as_u64().unwrap_or(0));
            // Taken off the list until it is saved again.
            if modified.is_some() && self.dismissed.get(&target) == modified.as_ref() {
                continue;
            }
            let doc = f["doc"].as_str().map(str::to_string);
            match targets.iter_mut().find(|t| t.target == target) {
                Some(t) => {
                    t.sources.push(src);
                    t.saved_modified = t.saved_modified.or(modified);
                    t.doc = t.doc.take().or(doc);
                }
                None => targets.push(Target {
                    doc,
                    saved_modified: modified,
                    target: target.clone(),
                    rel: f["rel"].as_str().unwrap_or_default().to_string(),
                    kind: f["kind"].as_str().unwrap_or("upk").to_string(),
                    script: f["script"].as_bool().unwrap_or(false),
                    sources: vec![src],
                    chosen: 0,
                    whole: f["base"]["kind"] == "new",
                    included: true,
                    base: match f["base"]["kind"].as_str() {
                        Some("recorded") => "compared with the original the Studio recorded".into(),
                        Some("backup") => "compared with the launcher's backup of the original".into(),
                        Some("new") => "a new package: shipped whole".into(),
                        _ => "compared with the game folder's file".into(),
                    },
                    preview: None,
                }),
            }
        }
        for t in &mut targets {
            // Unsaved edits first, then the newest copy, then the in-place save.
            let rank = |o: &str| match o {
                "unsaved" => 0,
                "copy" => 1,
                _ => 2,
            };
            t.chosen = (0..t.sources.len()).min_by_key(|&k| rank(&t.sources[k].2)).unwrap_or(0);
            if let Some((_, inc, whole)) = old.iter().find(|o| o.0 == t.target) {
                t.included = *inc;
                t.whole = *whole;
            }
            // A copy just added ships.
            if let Some(k) = t.sources.iter().position(|s| s.2 == "copy" && self.choose.iter().any(|c| super::project::same_path(c, &s.1))) {
                t.chosen = k;
                t.included = true;
            }
        }
        self.targets = targets;
        self.choose.clear();
    }

    fn item(t: &Target) -> J {
        let (id, source, _) = &t.sources[t.chosen];
        json!({ "id": id, "source": source, "target": t.target, "kind": t.kind, "mode": if t.whole { "whole" } else { "delta" } })
    }

    fn preview(&mut self, k: usize, cx: &mut Context<Self>) {
        let Some(t) = self.targets.get(k) else { return };
        let call = self.backend.call("mod.preview", json!({ "items": [Self::item(t)] }));
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        if let Some(t) = this.targets.get_mut(k) {
                            t.preview = r.json["items"].as_array().and_then(|a| a.first()).cloned();
                        }
                    }
                    // Shown in the file's card: previews run by themselves, a toast each would pile up.
                    Err(e) => {
                        if let Some(t) = this.targets.get_mut(k) {
                            t.preview = Some(json!({ "error": e }));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// What the mod ships besides the game files: added files and the project's mod-files/.
    fn ships_code(&self) -> bool {
        let code = |p: &str| {
            let l = p.to_ascii_lowercase();
            l.ends_with(".dll") || (l.ends_with(".lua") && !l.ends_with(".d.lua"))
        };
        let mut files: Vec<String> = self.extras.iter().map(|e| e.0.clone()).collect();
        if let Some(p) = &self.project {
            files.extend(walk(&p.mod_files()).into_iter().map(|f| f.to_string_lossy().to_string()));
        }
        files.iter().any(|f| code(f))
    }

    fn ships_lua(&self) -> bool {
        let lua = |p: &str| p.to_ascii_lowercase().ends_with(".lua");
        self.extras.iter().any(|e| lua(&e.0)) || self.project.as_ref().is_some_and(|p| walk(&p.mod_files().join("scripts")).iter().any(|f| lua(&f.to_string_lossy())))
    }

    /// Why Build cannot run yet (the launcher's and the repository's checks).
    pub(super) fn problems(&self, cx: &App) -> Vec<String> {
        let mut p = Vec::new();
        if self.meta.text("name", cx).is_empty() {
            p.push("Give the mod a name".into());
        }
        let id = self.mod_id(cx);
        // The id comes from the name: its rule only matters once there is one.
        if p.is_empty() && !modmeta::id_ok(&id) {
            p.push("The id is 2 to 64 lower-case letters, digits, dots, dashes and underscores, starting with a letter or digit".into());
        }
        let v = self.meta.text("version", cx);
        if !v.is_empty() && !modmeta::is_semver(&v) {
            p.push(format!("The version is three numbers like 1.0.0, optionally -beta.1 (\"{v}\" is not)"));
        }
        for d in self.meta.deps(cx) {
            if let Some(why) = modmeta::dependency_problem(&d) {
                p.push(format!("Dependency: {why}"));
            }
        }
        if let Some(icon) = &self.meta.icon {
            match std::fs::read(icon) {
                Ok(data) => p.extend(modmeta::icon_problem(&data)),
                Err(_) => p.push(format!("The icon {} is not there any more", file_name(icon))),
            }
        }
        if self.meta.text("summary", cx).chars().count() > modmeta::MAX_SUMMARY {
            p.push(format!("The summary is longer than {} characters", modmeta::MAX_SUMMARY));
        }
        let project_files = self.project.as_ref().is_some_and(|p| !walk(&p.mod_files()).is_empty());
        if !self.targets.iter().any(|t| t.included) && self.extras.is_empty() && !project_files {
            p.push("Nothing to ship: edit a package or a table first, or add files".into());
        }
        if self.ships_code() && !signing_key().exists() {
            p.push("The mod ships code (Lua or DLLs): create your signing key under Signing and publishing, or the launcher refuses it".into());
        }
        p
    }

    /// Build, asking first when the project already has a package of this version.
    fn build(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (id, version) = (self.mod_id(cx), self.version(cx));
        if let Some(out) = self.project.as_ref().map(|p| p.builds().join(format!("{id}-{version}.tempest"))).filter(|o| o.exists()) {
            let me = cx.entity().downgrade();
            super::dialogs::confirm(window, cx, format!("Replace the build of {version}?"), format!("{} already exists. Build again over it, or change the version first (a version already published cannot be uploaded again).", file_name(&out.to_string_lossy())), "Build again", false, move |_, cx| {
                let _ = me.update(cx, |this, cx| this.build_now(cx));
            });
            return;
        }
        self.build_now(cx);
    }

    pub(super) fn build_now(&mut self, cx: &mut Context<Self>) {
        if !self.problems(cx).is_empty() {
            return;
        }
        let (id, version) = (self.mod_id(cx), self.version(cx));
        let docs = std::env::var_os("HOME").map(|h| Path::new(&h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        // A project builds into its builds/ folder; Build & install of an unnamed
        // mod into a temporary one; otherwise ask where.
        let fixed = self.project.as_ref().map(|p| p.builds().join(format!("{id}-{version}.tempest"))).or_else(|| self.install_after.then(|| std::env::temp_dir().join("tempest-studio-native").join("builds").join(format!("{id}-{version}.tempest"))));
        let target = fixed.is_none().then(|| cx.prompt_for_new_path(&docs, Some(&format!("{id}-{version}.tempest"))));
        let mut extras: Vec<J> = self.extras.iter().map(|(s, d)| json!({ "src": s, "dest": d })).collect();
        extras.extend(self.project.as_ref().map(|p| p.mod_files()).filter(|m| m.is_dir()).map(|m| json!({ "src": m.to_string_lossy(), "dest": "" })));
        let name = self.meta.text("name", cx);
        let author = self.meta.text("author", cx);
        let link = self.meta.text("link", cx);
        let readme = self.meta.readme.read(cx).value().to_string();
        let icon = self.meta.icon.clone();
        let compression = if self.meta.fast { "fast" } else { "small" };
        let items: Vec<J> = self.targets.iter().filter(|t| t.included).map(Self::item).collect();
        let mut deps = self.meta.deps(cx);
        // Lua needs the scripting runtime.
        if self.ships_lua() && !deps.iter().any(|d| d.to_ascii_lowercase().starts_with("tempest-scripting")) {
            deps.push("tempest-scripting>=1.0.0".into());
        }
        cx.spawn(async move |this, cx| {
            let out = match (fixed, target) {
                (Some(f), _) => {
                    let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
                    f
                }
                (None, Some(t)) => {
                    let Ok(Ok(Some(o))) = t.await else {
                        let _ = this.update(cx, |this, _| this.install_after = false);
                        return;
                    };
                    o
                }
                _ => return,
            };
            let out = out.to_string_lossy().to_string();
            let staging = std::env::temp_dir().join("tempest-studio-native").join("staging").join(&id);
            let _ = std::fs::remove_dir_all(&staging);
            let _ = std::fs::create_dir_all(&staging);
            let Ok(call) = this.update(cx, |this, cx| {
                this.busy = Some("Cooking…".into());
                this.log.clear();
                this.built = None;
                this.say(Line::Info, format!("Cooking {} file{}…", items.len(), if items.len() == 1 { "" } else { "s" }), cx);
                let authors = if author.is_empty() { json!([]) } else { json!([{ "name": author, "link": link }]) };
                this.backend.call("mod.cook", json!({ "dir": staging.to_string_lossy(), "id": id, "name": name, "version": version, "authors": authors, "readme": readme, "icon": icon, "dependencies": deps, "items": items, "compression": compression, "extras": extras }))
            }) else { return };
            let r = call.await;
            let cooked = this.update(cx, |this, cx| match r {
                Ok(r) => {
                    for e in r.json["entries"].as_array().into_iter().flatten() {
                        let rel = e["rel"].as_str().unwrap_or_default();
                        let text = match e["mode"].as_str() {
                            Some("delta") => format!("{rel}: delta, {} changed, {} added, {} removed, {}", e["changed"].as_u64().unwrap_or(0), e["added"].as_u64().unwrap_or(0), e["deleted"].as_u64().unwrap_or(0), crate::model::human_size(e["bytes"].as_u64().unwrap_or(0))),
                            Some("unchanged") => format!("{rel}: no changes, left out"),
                            Some(m) => format!("{rel}: {m}"),
                            None => rel.to_string(),
                        };
                        this.say(Line::Ok, text, cx);
                        for w in e["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()) {
                            this.say(Line::Info, format!("  {w}"), cx);
                        }
                    }
                    if let Some(saved) = r.json["shared"]["saved"].as_u64().filter(|s| *s > 0) {
                        this.say(Line::Ok, format!("Objects changed the same way in several files are stored once: {} shared, {:.1} MB saved", r.json["shared"]["files"], saved as f64 / 1048576.0), cx);
                    }
                    true
                }
                Err(e) => {
                    this.say(Line::Error, format!("Cooking failed: {e}"), cx);
                    this.busy = None;
                    this.install_after = false;
                    false
                }
            });
            if !matches!(cooked, Ok(true)) {
                return;
            }
            let (s, o) = (staging.to_string_lossy().to_string(), out.clone());
            let packed = cx.background_executor().spawn(async move { run_cli(&["pack".into(), s, o]) }).await;
            let signed = match &packed {
                Ok((0, _)) if signing_key().exists() => {
                    let o = out.clone();
                    Some(cx.background_executor().spawn(async move { run_cli(&["sign".into(), o]) }).await)
                }
                _ => None,
            };
            let signed_ok = matches!(signed, Some(Ok((0, _))));
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                let install = std::mem::take(&mut this.install_after);
                match packed {
                    Ok((0, _)) => {}
                    Ok((_, text)) => {
                        this.say(Line::Error, format!("tempest mod pack failed: {}", text.trim()), cx);
                        return;
                    }
                    Err(e) => {
                        this.say(Line::Error, e, cx);
                        return;
                    }
                }
                match signed {
                    Some(Ok((0, text))) => this.say(Line::Ok, format!("Signed{}", text.lines().find_map(|l| l.split_once(" with ").map(|(_, w)| format!(" with {}", w.trim()))).unwrap_or_default()), cx),
                    Some(Ok((_, text))) => this.say(Line::Error, format!("Signing failed: {}", text.trim()), cx),
                    Some(Err(e)) => this.say(Line::Error, e, cx),
                    None => this.say(Line::Info, "No signing key: the package is unsigned (fine for assets, refused for code)", cx),
                }
                this.say(Line::Ok, format!("Built {}", out), cx);
                this.built = Some((out.clone(), signed_ok));
                if install {
                    this.install_here(cx);
                } else {
                    this.toast(Tone::Success, format!("Mod built: {}", file_name(&out)), cx);
                }
            });
        })
        .detach();
    }

    /// Install the built package into this game folder; files saved in place here already hold the edits, so it asks first.
    fn install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.built.is_none() {
            return;
        }
        let game = self.game_root();
        let saved_here: Vec<String> = self.targets.iter().filter(|t| t.included && t.sources.iter().any(|s| s.2 == "saved")).map(|t| t.rel.clone()).collect();
        if saved_here.is_empty() {
            self.install_into(game, cx);
            return;
        }
        let me = cx.entity().downgrade();
        super::dialogs::confirm(window, cx, "This game folder already has your changes", format!("These files were saved in place, so they already hold your changes: {}. The mod would be applied on top of them, and testing here would not show that the mod works on a clean game. Install into another copy of the game (Install elsewhere…) to test it properly.", saved_here.join(", ")), "Install here anyway", true, move |_, cx| {
            let game = game.clone();
            let _ = me.update(cx, |this, cx| this.install_into(game, cx));
        });
    }

    /// The folder holding ChaosGame.
    fn game_root(&self) -> String {
        Path::new(&self.dir).parent().and_then(|p| p.parent()).map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
    }

    /// Build & install: a fast build, then into this game.
    fn build_and_install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.install_after = true;
        self.meta.fast = true;
        self.build(window, cx);
    }

    /// After Build & install: into this game, unless files saved in place would hide whether the mod works.
    fn install_here(&mut self, cx: &mut Context<Self>) {
        let saved_here = self.targets.iter().any(|t| t.included && t.sources.iter().any(|s| s.2 == "saved"));
        if saved_here {
            self.say(Line::Info, "Not installed: some game files here were overwritten with your changes, so this copy cannot show whether the mod works. Use ⋯ ▸ Install elsewhere, or Install here anyway.", cx);
            self.toast(Tone::Warning, "Built, not installed: this game folder already has your changes", cx);
            return;
        }
        let game = self.game_root();
        self.install_into(game, cx);
    }

    /// Take the mod out of this game (`tempest mod remove`, by the name it was installed under).
    fn remove_from_game(&mut self, cx: &mut Context<Self>) {
        let (game, id) = (self.game_root(), self.mod_id(cx));
        self.busy = Some("Removing…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (g, i) = (game.clone(), id.clone());
            let r = cx
                .background_executor()
                .spawn(async move {
                    let (_, out, _) = run_cli_split(&["list".into(), g.clone(), "--json".into()])?;
                    let list: J = serde_json::from_str(out.trim()).map_err(|e| format!("tempest mod list: {e}"))?;
                    let Some(name) = installed_name(&list, &i) else { return Ok(None) };
                    run_cli(&["remove".into(), g, name.clone()]).map(|(code, text)| Some((code, text, name)))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match r {
                    Ok(None) => this.toast(Tone::Info, "This mod is not installed in this game", cx),
                    Ok(Some((0, _, name))) => {
                        this.say(Line::Ok, format!("Removed {name} from {game}"), cx);
                        this.toast(Tone::Success, "Mod removed from the game", cx);
                    }
                    Ok(Some((_, text, _))) => this.say(Line::Error, format!("Could not remove the mod: {}", text.trim()), cx),
                    Err(e) => this.say(Line::Error, e, cx),
                }
            });
        })
        .detach();
    }

    /// Install the built package into another game folder (the one holding ChaosGame).
    fn install_elsewhere(&mut self, cx: &mut Context<Self>) {
        let pick = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Install into this game folder".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut dirs))) = pick.await else { return };
            let Some(dir) = dirs.pop() else { return };
            let _ = this.update(cx, |this, cx| this.install_into(dir.to_string_lossy().to_string(), cx));
        })
        .detach();
    }

    fn install_into(&mut self, game: String, cx: &mut Context<Self>) {
        let Some((pkg, _)) = self.built.clone() else { return };
        self.busy = Some("Installing…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let g = game.clone();
            let r = cx.background_executor().spawn(async move { run_cli(&["install".into(), g, pkg, "--replace".into(), "--allow-untrusted".into()]) }).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match r {
                    Ok((0, _)) => {
                        this.say(Line::Ok, format!("Installed into {game}. Play it from Tempest Launcher to test it."), cx);
                        this.toast(Tone::Success, "Mod installed: play from Tempest Launcher to test it", cx);
                    }
                    Ok((_, text)) => this.say(Line::Error, format!("Install failed: {}", text.trim()), cx),
                    Err(e) => this.say(Line::Error, e, cx),
                }
            });
        })
        .detach();
    }

    /// Dev: `cooker <step>` of the test driver.
    pub fn drive(&mut self, arg: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let (verb, rest) = arg.split_once(' ').unwrap_or((arg, ""));
        match verb {
            "refresh" => self.refresh(cx),
            "build" => self.build_now(cx),
            // Opens the dialog only: publishing is left to a person.
            "publish-dialog" => self.publish_dialog(window, cx),
            "preview" => self.preview(rest.trim().parse().map_err(|_| "preview <n>")?, cx),
            "name" => {
                let v = rest.trim().to_string();
                self.meta.name.update(cx, |s, cx| s.set_value(v, window, cx));
            }
            _ => return Err(format!("unknown cooker step {verb}")),
        }
        cx.notify();
        Ok(())
    }

    fn render_files(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut list = v_flex().gap_2();
        if self.targets.is_empty() {
            list = list.child(div().text_sm().text_color(theme.muted_foreground).child(if self.loading { "Looking for your changes…" } else { "No changes yet. Edit a package or an assembly table, and it shows up here: edits, files written in place and exported copies all count." }));
        }
        for (k, t) in self.targets.iter().enumerate() {
            let (id, _, origin) = &t.sources[t.chosen];
            let origin = match origin.as_str() {
                "unsaved" => "changes in the Studio",
                "copy" => "an exported copy",
                _ => "saved in the game folder",
            };
            let _ = id;
            let mut card = v_flex()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .when(!t.included, |d| d.opacity(0.5))
                .child(
                    h_flex()
                        .gap_2()
                        .child(Checkbox::new(("inc", k)).checked(t.included).on_click(cx.listener(move |this, v: &bool, _, cx| {
                            if let Some(t) = this.targets.get_mut(k) {
                                t.included = *v;
                            }
                            cx.notify();
                        })))
                        .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_sm().child(h_flex().gap_2().min_w_0().child(div().flex_none().font_weight(gpui::FontWeight::SEMIBOLD).child(file_in_words(&t.rel))).child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().text_color(theme.muted_foreground).child(t.rel.clone()))))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(format!("from {origin}")))
                        .child({
                            let b = Button::new(("delta", k)).xsmall().label("Only the changes").tooltip("A delta: just the objects or rows you changed, so other mods of this file still work").on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(t) = this.targets.get_mut(k) {
                                    t.whole = false;
                                    t.preview = None;
                                }
                                cx.notify();
                            }));
                            b.ghost().selected(!t.whole)
                        })
                        .when(!t.script, |d| {
                            d.child({
                                let b = Button::new(("whole", k)).xsmall().label("Whole file").tooltip("Replace the game's file: bigger, and two mods of this file conflict").on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(t) = this.targets.get_mut(k) {
                                        t.whole = true;
                                        t.preview = None;
                                    }
                                    cx.notify();
                                }));
                                b.ghost().selected(t.whole)
                            })
                        })
                        .child(Button::new(("prev", k)).xsmall().ghost().label("What changed").on_click(cx.listener(move |this, _, _, cx| this.preview(k, cx))))
                        .child({
                            let me = cx.entity();
                            div().relative().child(Button::new(("more", k)).xsmall().ghost().label("…").tooltip("More: discard, remove from the list, forget a copy").dropdown_menu(move |menu, _, cx| me.update(cx, |c, cx| c.target_menu(k, menu, cx)))).child(crate::driver::probe(format!("more-{k}")))
                        }),
                )
                .child(div().text_xs().text_color(theme.muted_foreground).child(t.base.clone()));
            if t.sources.len() > 1 {
                let mut pick = h_flex().gap_1().child(div().text_xs().text_color(theme.muted_foreground).child("ship"));
                for (j, s) in t.sources.iter().enumerate() {
                    let label = match s.2.as_str() {
                        "unsaved" => "changes in the Studio".to_string(),
                        "copy" => format!("copy {}", file_name(&s.1)),
                        _ => "saved file".to_string(),
                    };
                    let b = Button::new(SharedString::from(format!("src-{k}-{j}"))).xsmall().label(label).on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(t) = this.targets.get_mut(k) {
                            t.chosen = j;
                            t.preview = None;
                        }
                        cx.notify();
                    }));
                    pick = pick.child(b.ghost().selected(j == t.chosen));
                }
                card = card.child(pick);
            }
            if let Some(p) = &t.preview {
                let summary = match p["mode"].as_str() {
                    Some("unchanged") => "No change from the original: left out of the mod".to_string(),
                    Some("whole") => format!("The whole file, {}", crate::model::human_size(p["bytes"].as_u64().unwrap_or(0))),
                    _ => format!("{} changed, {} added, {} removed", p["changed"].as_u64().unwrap_or(0), p["added"].as_u64().unwrap_or(0), p["deleted"].as_u64().unwrap_or(0)),
                };
                let mut changes = v_flex().gap_0p5().child(div().text_xs().child(summary));
                if let Some(e) = p["error"].as_str() {
                    changes = changes.child(div().text_xs().text_color(theme.danger).child(e.to_string()));
                }
                for c in p["changes"].as_array().into_iter().flatten().take(60) {
                    changes = changes.child(h_flex().gap_2().text_xs().child(div().w(px(60.0)).text_color(theme.muted_foreground).child(c["op"].as_str().unwrap_or_default().to_string())).child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(c["what"].as_str().unwrap_or_default().to_string())));
                }
                for w in p["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()) {
                    changes = changes.child(div().text_xs().text_color(theme.warning).child(w.to_string()));
                }
                card = card.child(changes);
            }
            list = list.child(card);
        }
        list.into_any_element()
    }
}

impl Cooker {
    fn render_icon(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let row = h_flex().gap_2().items_center();
        match self.meta.icon.clone() {
            Some(icon) => row
                .child(gpui::img(PathBuf::from(&icon)).size(px(48.0)).rounded_md())
                .child(div().flex_1().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(file_name(&icon)))
                .child(Button::new("icon-change").xsmall().ghost().label("Change…").on_click(cx.listener(|this, _, _, cx| this.pick_icon(cx))))
                .child(Button::new("icon-remove").xsmall().ghost().label("Remove").on_click(cx.listener(|this, _, _, cx| {
                    this.meta.icon = None;
                    this.save_meta(cx);
                    cx.notify();
                })))
                .into_any_element(),
            None => row
                .child(Button::new("icon-pick").xsmall().label("Choose an icon…").on_click(cx.listener(|this, _, _, cx| this.pick_icon(cx))))
                .child(div().flex_1().min_w_0().text_xs().text_color(theme.muted_foreground).child("PNG, 1 MB at most"))
                .into_any_element(),
        }
    }

    fn pick_icon(&mut self, cx: &mut Context<Self>) {
        let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("The mod's icon (PNG)".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut p))) = files.await else { return };
            let Some(f) = p.pop() else { return };
            let _ = this.update(cx, |this, cx| {
                let f = f.to_string_lossy().to_string();
                if let Some(why) = std::fs::read(&f).ok().and_then(|d| modmeta::icon_problem(&d)) {
                    this.toast(Tone::Warning, why, cx);
                    return;
                }
                this.meta.icon = Some(f);
                this.save_meta(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Other files the mod ships as they are, and the buttons that add them.
    fn render_extras(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut list = v_flex().gap_1();
        for (k, (src, dest)) in self.extras.iter().enumerate() {
            list = list.child(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .child(div().font_family(crate::studio::ui::mono()).text_xs().child(dest.clone()))
                    .child(div().flex_1().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(format!("from {src}")))
                    .child(Button::new(("extra-rm", k)).xsmall().ghost().label("Remove").on_click(cx.listener(move |this, _, _, cx| {
                        if k < this.extras.len() {
                            this.extras.remove(k);
                        }
                        this.save_meta(cx);
                        cx.notify();
                    }))),
            );
        }
        v_flex()
            .mt_2()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("Other files"))
                    .child(Button::new("add-files").xsmall().ghost().label("Add files…").tooltip("Lua goes to scripts/, DLLs to dlls/64/; .upk, .dat and .pck files become modified game files").on_click(cx.listener(|this, _, _, cx| this.add_files(cx))))
                    .child(Button::new("add-folder").xsmall().ghost().label("Add folder…").tooltip("A whole folder, shipped under its name (for example scripts/)").on_click(cx.listener(|this, _, _, cx| this.add_folder(cx))))
                    .child(Button::new("add-modified").xsmall().ghost().label("Add a modified game file…").tooltip("A package, .dat or .pck edited elsewhere: shipped as a delta against the game's file").on_click(cx.listener(|this, _, _, cx| this.add_files(cx)))),
            )
            .when(self.extras.is_empty(), |d| d.child(div().text_xs().text_color(theme.muted_foreground).child("Scripts, DLLs or files edited outside the Studio can ship too.")))
            .child(list)
            .into_any_element()
    }

    /// Ship a whole folder in the mod, under its name.
    fn add_folder(&mut self, cx: &mut Context<Self>) {
        let pick = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Ship this folder".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut dirs))) = pick.await else { return };
            let Some(dir) = dirs.pop() else { return };
            let _ = this.update(cx, |this, cx| {
                let src = dir.to_string_lossy().to_string();
                if !this.extras.iter().any(|e| e.0 == src) {
                    this.extras.push((src, dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
                    this.save_meta(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn add_files(&mut self, cx: &mut Context<Self>) {
        let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Files to ship in the mod".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(picked))) = files.await else { return };
            let _ = this.update(cx, |this, cx| {
                for f in picked {
                    let f = f.to_string_lossy().to_string();
                    let lower = f.to_ascii_lowercase();
                    if lower.ends_with(".upk") || lower.ends_with(".dat") || lower.ends_with(".pck") {
                        this.add_modified(f, cx);
                    } else if !this.extras.iter().any(|e| e.0 == f) {
                        let dest = default_dest(&f);
                        this.extras.push((f, dest));
                    }
                }
                this.save_meta(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// A game file edited elsewhere: registered as a copy of the game file of its name (`mod.match`), so it ships as a delta.
    fn add_modified(&mut self, file: String, cx: &mut Context<Self>) {
        let call = self.backend.call("mod.match", json!({ "path": file }));
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let target = call.await.ok().and_then(|r| r.json["target"].as_str().map(str::to_string));
            let Some(target) = target else {
                // No game file of that name: which one it is a modified version of, or a new file.
                let _ = this.update(cx, |this, cx| {
                    this.unmatched = Some(file.clone());
                    cx.notify();
                });
                return;
            };
            let added = backend.call("mod.addCopy", json!({ "path": file, "target": target })).await;
            let _ = this.update(cx, |this, cx| match added {
                Ok(_) => {
                    this.choose.push(file.clone());
                    if let Some(name) = Some(file_name(&target).to_ascii_lowercase()) {
                        let _ = this.studio.update(cx, |s, _| {
                            if let Some(p) = &mut s.project {
                                p.note_files([name]);
                            }
                        });
                    }
                    this.toast(Tone::Success, format!("{} ships as a modified {}", file_name(&file), file_name(&target)), cx);
                    this.refresh(cx);
                }
                Err(e) => this.toast(Tone::Error, e, cx),
            });
        })
        .detach();
    }

    /// The `…` menu of a game file of the list.
    fn target_menu(&self, k: usize, menu: gpui_component::menu::PopupMenu, cx: &mut Context<Self>) -> gpui_component::menu::PopupMenu {
        let Some(t) = self.targets.get(k) else { return menu };
        let me = cx.entity().downgrade();
        type Action = dyn Fn(&mut Cooker, &mut Window, &mut Context<Cooker>);
        let item = |label: String, f: Box<Action>| {
            let me = me.clone();
            let f: std::rc::Rc<Action> = f.into();
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                let f = f.clone();
                let _ = me.update(cx, |this, cx| f(this, window, cx));
            })
        };
        let unsaved = t.sources.iter().find(|s| s.2 == "unsaved").map(|s| s.1.clone());
        let mut menu = menu
            .item(item("Show the changes".into(), Box::new(move |this, _, cx| this.preview(k, cx))))
            .item(item(if t.included { "Leave it out of the mod" } else { "Put it in the mod" }.into(), Box::new(move |this, _, cx| {
                if let Some(t) = this.targets.get_mut(k) {
                    t.included = !t.included;
                }
                cx.notify();
            })));
        if !t.script {
            menu = menu.item(item(if t.whole { "Ship only the changes" } else { "Ship the whole file" }.into(), Box::new(move |this, _, cx| {
                if let Some(t) = this.targets.get_mut(k) {
                    t.whole = !t.whole;
                    t.preview = None;
                }
                cx.notify();
            })));
        }
        menu = menu.separator();
        if let Some(path) = unsaved {
            let doc = t.doc.clone();
            menu = menu.item(item("Discard my edits…".into(), Box::new(move |this, window, cx| this.discard_edits(path.clone(), doc.clone(), window, cx))));
        }
        for (_, src, origin) in t.sources.iter().filter(|s| s.2 == "copy") {
            let src = src.clone();
            let _ = origin;
            menu = menu.item(item(format!("Forget the copy {}", file_name(&src)), Box::new(move |this, _, cx| this.forget_copy(src.clone(), cx))));
        }
        // Copies are forgotten above; unsaved edits are discarded; what is left is a file saved in place.
        if self.project.is_some() {
            menu.item(item("Remove it from the project".into(), Box::new(move |this, _, cx| this.dismiss(k, cx))))
        } else if t.saved_modified.is_some() {
            menu.item(item("Remove it from the list (until saved again)".into(), Box::new(move |this, _, cx| this.dismiss(k, cx))))
        } else {
            menu
        }
    }

    fn discard_edits(&mut self, path: String, doc: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        match doc {
            // Assembly files: the backend reloads the document, the Assembly view reads it again.
            Some(doc) => {
                let me = cx.entity().downgrade();
                super::dialogs::confirm(window, cx, format!("Discard your changes to {}?", file_name(&path)), "The edits you made in the Studio are lost and the file is read again from the game folder. Copies you exported are not touched.", "Discard edits", true, move |_, cx| {
                    let doc = doc.clone();
                    let _ = me.update(cx, |this, cx| {
                        let call = this.backend.call("mcts.discard", json!({ "doc": doc }));
                        cx.spawn(async move |this, cx| {
                            let r = call.await;
                            let _ = this.update(cx, |this, cx| {
                                if let Err(e) = r {
                                    this.toast(Tone::Error, e, cx);
                                }
                                let _ = this.studio.update(cx, |s, cx| {
                                    if let Some(a) = s.assembly_entity() {
                                        a.update(cx, |a, cx| a.reopen(cx));
                                    }
                                });
                                this.refresh(cx);
                            });
                        })
                        .detach();
                    });
                });
            }
            None => {
                let _ = self.studio.update(cx, |s, cx| s.discard(path, window, cx));
            }
        }
    }

    fn forget_copy(&mut self, path: String, cx: &mut Context<Self>) {
        let call = self.backend.call("mod.forgetCopy", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            let _ = call.await;
            let _ = this.update(cx, |this, cx| this.refresh(cx));
        })
        .detach();
    }

    /// Off the list: a project forgets the file; an in-place save stays hidden until it is saved again.
    fn dismiss(&mut self, k: usize, cx: &mut Context<Self>) {
        if k >= self.targets.len() {
            return;
        }
        let t = self.targets.remove(k);
        if let Some(m) = t.saved_modified {
            self.dismissed.insert(t.target.clone(), m);
        }
        let name = file_name(&t.target).to_ascii_lowercase();
        let _ = self.studio.update(cx, |s, _| {
            if let Some(p) = &mut s.project {
                p.files.retain(|f| *f != name);
            }
        });
        self.save_meta(cx);
        cx.notify();
    }

    /// The project's mod-files/ (shipped as laid out), or the suggestion to start a project.
    fn render_project_files(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(p) = &self.project else {
            if self.meta.text("name", cx).is_empty() {
                return div().into_any_element();
            }
            return h_flex()
                .mt_2()
                .p_2()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .child(div().flex_1().text_xs().text_color(theme.muted_foreground).child("Name this mod to give it its own folder: it saves itself as you work, can ship scripts and other files, and keeps its builds."))
                .child(Button::new("cook-new-project").xsmall().label("Name this mod…").on_click(cx.listener(|this, _, window, cx| {
                    let _ = this.studio.update(cx, |s, cx| s.run(super::Cmd::NewProject, window, cx));
                })))
                .into_any_element();
        };
        let files = p.mod_files();
        let mut n = 0usize;
        let mut stack = vec![files.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                if e.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                if e.path().is_dir() {
                    stack.push(e.path());
                } else {
                    n += 1;
                }
            }
        }
        h_flex()
            .mt_2()
            .p_2()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .flex_1()
                    .child(div().text_sm().child(format!("mod-files/: {n} file{}", if n == 1 { "" } else { "s" })))
                    .child(div().text_xs().text_color(theme.muted_foreground).child("Shipped as laid out: scripts/ (Lua), dlls/64/ and dlls/32/, files/<game path>.")),
            )
            .child(Button::new("cook-mod-files").xsmall().ghost().label("Open the folder").on_click(move |_, _, cx| cx.open_with_system(&files)))
            .into_any_element()
    }
}

/// Every file under `dir` (dotfiles left out).
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path());
            }
        }
    }
    out
}

/// Where an added file goes inside the mod (Lua → scripts/, DLLs → dlls/64/).
fn default_dest(src: &str) -> String {
    let name = file_name(src);
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".lua") {
        format!("scripts/{name}")
    } else if lower.ends_with(".dll") {
        format!("dlls/64/{name}")
    } else {
        format!("files/{name}")
    }
}

fn meta_file(dir: &str) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut h);
    state_file(&format!("mod-{:016x}.json", h.finish()))
}

impl Render for Cooker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(file) = self.unmatched.take() {
            self.ask_unmatched(file, window, cx);
        }
        let edits = crate::backend::EDITS.load(std::sync::atomic::Ordering::SeqCst);
        if edits != self.seen && !self.loading {
            self.seen = edits;
            self.refresh(cx);
        }
        let theme = cx.theme().clone();
        let files = self.render_files(cx);
        let problems = self.problems(cx);
        let id = self.mod_id(cx);
        let section = |t: &str| div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(t.to_string());
        let labeled = |label: &'static str, input: &Entity<InputState>| v_flex().gap_1().child(div().text_xs().text_color(theme.muted_foreground).child(label)).child(Input::new(input).small());
        let busy = self.busy.clone();
        let mut log = v_flex().gap_0p5();
        for (line, text) in &self.log {
            log = log.child(div().text_xs().font_family(crate::studio::ui::mono()).text_color(match line {
                Line::Ok => theme.foreground,
                Line::Info => theme.muted_foreground,
                Line::Error => theme.danger,
            }).child(text.clone()));
        }
        let cli = cli_path();
        let v = self.meta.text("version", cx);
        let fix = (!v.is_empty() && !modmeta::is_semver(&v)).then(|| modmeta::to_semver(&v)).flatten();
        let summary_len = self.meta.text("summary", cx).chars().count();
        let lua = self.ships_lua();
        let account = self.render_account(cx);
        let extras = self.render_extras(cx);
        let built = self.built.clone();
        let blocked = !problems.is_empty() || busy.is_some() || cli.is_none();
        let included = self.targets.iter().filter(|t| t.included).count() + self.extras.len();
        let name = self.meta.text("name", cx);
        let title = if name.is_empty() { self.project.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "Unnamed mod".into()) } else { name };
        let me = cx.entity().downgrade();
        let can_use = built.is_some() && busy.is_none();
        let unnamed = self.project.is_none() && self.meta.text("name", cx).is_empty();
        // The bar: the mod, what is wrong, and Build & install.
        let bar = h_flex()
            .flex_none()
            .px_4()
            .py_3()
            .gap_3()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(div().flex_none().size(px(40.0)).rounded_lg().bg(theme.secondary).flex().items_center().justify_center().child(gpui_component::Icon::new(gpui_kit_assets::IconName::Hammer).text_color(theme.muted_foreground)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_lg().font_weight(gpui::FontWeight::BOLD).overflow_hidden().text_ellipsis().whitespace_nowrap().child(title))
                    .child(div().text_xs().text_color(if problems.is_empty() { theme.muted_foreground } else { theme.warning }).overflow_hidden().text_ellipsis().whitespace_nowrap().child(match (problems.first(), &built) {
                        (Some(p), _) => format!("Before building: {p}"),
                        (None, Some((path, signed))) => format!("Built {}{} · version {}", file_name(path), if *signed { ", signed" } else { "" }, if v.is_empty() { "1.0.0".into() } else { v.clone() }),
                        (None, None) => format!("{included} file{} go{} in · version {}", if included == 1 { "" } else { "s" }, if included == 1 { "es" } else { "" }, if v.is_empty() { "1.0.0".into() } else { v.clone() }),
                    })),
            )
            // An unnamed mod's first step: its name (and its own folder).
            .when(unnamed, |d| {
                d.child(Button::new("cook-name").small().primary().label("Name this mod…").on_click(cx.listener(|this, _, window, cx| {
                    let _ = this.studio.update(cx, |s, cx| s.run(super::Cmd::NewProject, window, cx));
                })))
            })
            .child(Button::new("cook-remove").small().ghost().label("Remove from game").tooltip(if id.is_empty() { "Name the mod first: it is removed by its id" } else { "Take this mod out of the game folder that is open" }).disabled(busy.is_some() || cli.is_none() || id.is_empty()).on_click(cx.listener(|this, _, _, cx| this.remove_from_game(cx))))
            .child(Button::new("cook-play").small().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Gamepad2)).label("Play in Tempest Launcher").tooltip("Tempest Launcher starts the game with your installed mods").on_click(|_, _, cx| cx.open_url("tempest://")))
            .child(
                Button::new("cook-more").small().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Ellipsis)).tooltip("Build only, save a copy, install elsewhere, publish").dropdown_menu(move |menu, _, _| {
                    let item = |label: &str, enabled: bool, f: fn(&mut Cooker, &mut Window, &mut Context<Cooker>)| {
                        let me = me.clone();
                        gpui_component::menu::PopupMenuItem::new(label.to_string()).disabled(!enabled).on_click(move |_, window, cx| {
                            let _ = me.update(cx, |this, cx| f(this, window, cx));
                        })
                    };
                    menu.item(item("Build only…", !blocked, |this, w, cx| this.build(w, cx)))
                        .item(item("Install here anyway", can_use, |this, w, cx| this.install(w, cx)))
                        .item(item("Install elsewhere…", can_use, |this, _, cx| this.install_elsewhere(cx)))
                        .item(item("Save a copy…", can_use, |this, _, cx| this.save_built_copy(cx)))
                        .separator()
                        .item(item("Publish…", can_use, |this, w, cx| this.publish_dialog(w, cx)))
                }),
            )
            .child(div().relative().child(Button::new("cook-build").when(!unnamed, |b| b.primary()).icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Hammer)).label(busy.clone().unwrap_or_else(|| "Build & install".into())).tooltip("Build the mod (fast) and install it into this game, ready to play from Tempest Launcher").disabled(blocked).on_click(cx.listener(|this, _, window, cx| this.build_and_install(window, cx)))).child(crate::driver::probe("cook-build")));
        let tab = self.tab;
        let tab_button = |id: &'static str, label: &'static str, t: ModTab, cx: &mut Context<Self>| {
            Button::new(id).small().ghost().label(label).selected(tab == t).on_click(cx.listener(move |this, _, _, cx| {
                this.tab = t;
                cx.notify();
            }))
        };
        let tabs = h_flex().flex_none().px_3().py_1().gap_1().border_b_1().border_color(theme.border).child(tab_button("cook-tab-changes", "Changes", ModTab::Changes, cx)).child(tab_button("cook-tab-details", "Details", ModTab::Details, cx)).child(tab_button("cook-tab-publish", "Publish", ModTab::Publish, cx));
        let page = v_flex().max_w(px(820.0)).w_full().p_4().gap_3();
        let page = match tab {
            ModTab::Changes => page
                .child(h_flex().gap_2().child(div().flex_1().child(section("What goes in"))).child(Button::new("cook-refresh").xsmall().ghost().label("Refresh").on_click(cx.listener(|this, _, _, cx| this.refresh(cx)))))
                .child(crate::studio::ui::hint("Every game file your changes touch. They go into the mod as they are in the Studio; the game's own files stay untouched.", &theme))
                .child(files)
                .child(extras)
                .child(self.render_project_files(cx))
                .when(!self.log.is_empty(), |d| d.child(section("Last build")).child(log))
                .children(problems.iter().skip(1).map(|p| div().text_xs().text_color(theme.warning).child(format!("• {p}"))))
                .when(cli.is_none(), |d| d.child(div().text_xs().text_color(theme.danger).child("The mod packer (tempest-cli) is missing from this Studio, so mods cannot be built. Reinstall the Studio, or set TEMPEST_CLI."))),
            ModTab::Details => page
                .child(section("Details"))
                .child(labeled("Name", &self.meta.name))
                .child(h_flex().gap_2().child(div().flex_1().child(labeled("Author", &self.meta.author))).child(div().flex_1().child(labeled("Author link", &self.meta.link))))
                .child(v_flex().gap_1().child(labeled("Version", &self.meta.version)).when_some(fix, |d, v| {
                    d.child(h_flex().gap_2().child(div().text_xs().text_color(theme.warning).child(format!("Not a version the launcher reads; did you mean {v}?"))).child(Button::new("cook-fix-version").xsmall().label(format!("Use {v}")).on_click(cx.listener(move |this, _, window, cx| {
                        let v = v.clone();
                        this.meta.version.update(cx, |s, cx| s.set_value(v, window, cx));
                        this.save_meta(cx);
                    }))))
                }))
                .child(section("Icon and readme"))
                .child(self.render_icon(cx))
                .child(Textarea::new(&self.meta.readme))
                .child(section("Advanced"))
                .child(v_flex().gap_1().child(labeled("Id", &self.meta.id)).child(crate::studio::ui::hint(format!("How launchers and other mods name this mod: {}. Made from the name unless you type one.", if id.is_empty() { "—".to_string() } else { id.clone() }), &theme)))
                .child(v_flex().gap_1().child(div().text_xs().text_color(theme.muted_foreground).child("Mods it needs")).child(Textarea::new(&self.meta.deps)).child(crate::studio::ui::hint(if lua { "tempest-scripting>=1.0.0 is added for you: the mod ships Lua." } else { "One per line: an id, then a version requirement like >=1.2.0, ^2 or ~1.2." }, &theme)))
                .child({
                    let pick = |id: &'static str, label: &'static str, tip: &'static str, fast: bool, on: bool, cx: &mut Context<Self>| {
                        Button::new(id).small().label(label).tooltip(tip).ghost().selected(on).on_click(cx.listener(move |this, _, _, cx| {
                            this.meta.fast = fast;
                            this.save_meta(cx);
                            cx.notify();
                        }))
                    };
                    let fast = self.meta.fast;
                    v_flex()
                        .gap_1()
                        .child(div().text_xs().text_color(theme.muted_foreground).child("Compression of Build only"))
                        .child(h_flex().gap_1().child(pick("cook-small", "Smallest file", "LZMA: the smallest mod, slower to build (a big mod takes a minute or so)", false, !fast, cx)).child(pick("cook-fast", "Faster build", "zstd: builds several times faster, the mod is about 30 % bigger", true, fast, cx)))
                        .child(crate::studio::ui::hint("Build & install always builds fast. Build the smallest file for sharing and publishing.", &theme))
                }),
            ModTab::Publish => page
                .child(section("Listing"))
                .child(crate::studio::ui::hint("How the mod shows in the mod list of Tempest Launcher once it is published.", &theme))
                .child(v_flex().gap_1().child(labeled("Summary", &self.meta.summary)).child(div().text_xs().text_color(if summary_len > modmeta::MAX_SUMMARY { theme.danger } else { theme.muted_foreground }).child(format!("{summary_len} / {} characters", modmeta::MAX_SUMMARY))))
                .child(labeled("Tags", &self.meta.tags))
                .child(labeled("Source link", &self.meta.repository))
                .child(h_flex().gap_2().child(Button::new("cook-publish").label("Publish…").disabled(self.built.is_none() || busy.is_some()).tooltip("Upload the built mod to the mod repository").on_click(cx.listener(|this, _, window, cx| this.publish_dialog(window, cx)))).when(self.built.is_none(), |d| d.child(crate::studio::ui::hint("Build the mod first (smallest file is best for publishing).", &theme))))
                .child(section("Signing and account"))
                .child(account),
        };
        v_flex().size_full().child(bar).child(tabs).child(div().id("cook-page").flex_1().min_h_0().overflow_y_scroll().child(page))
    }
}

impl Cooker {
    /// For the MCP server: what goes in, the details, the problems, the last build.
    pub(super) fn summary(&self, cx: &App) -> J {
        json!({
            "loading": self.loading,
            "files": self.targets.iter().map(|t| json!({ "file": t.rel, "included": t.included, "whole": t.whole, "sources": t.sources.iter().map(|s| s.2.clone()).collect::<Vec<_>>(), "base": t.base })).collect::<Vec<_>>(),
            "extras": self.extras.iter().map(|(s, d)| json!({ "src": s, "dest": d })).collect::<Vec<_>>(),
            "name": self.meta.text("name", cx),
            "id": self.mod_id(cx),
            "version": self.version(cx),
            "problems": self.problems(cx),
            "busy": self.busy,
            "built": self.built.as_ref().map(|(p, signed)| json!({ "path": p, "signed": signed })),
            "log": self.log.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
        })
    }
}

impl Cooker {
    /// A modified file whose name no game file has: pick the game file, or ship it as a new one.
    fn ask_unmatched(&mut self, file: String, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt;
        let me = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let (m1, m2, f1, f2) = (me.clone(), me.clone(), file.clone(), file.clone());
            dialog
                .title(format!("Which game file is {}?", file_name(&file)))
                .w(px(560.0))
                .child(div().text_sm().text_color(theme.muted_foreground).child("No file of this name is in the game folder. Pick the game file it is a modified version of (the mod then ships only the differences), or ship it as a new file."))
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(Button::new("um-cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
                        .child(Button::new("um-new").label("Ship it as a new file").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let f = f1.clone();
                            let _ = m1.update(cx, |this, cx| {
                                let dest = format!("files/ChaosGame/CookedPCConsole/{}", file_name(&f));
                                this.extras.push((f.clone(), dest));
                                this.save_meta(cx);
                                cx.notify();
                            });
                        }))
                        .child(Button::new("um-pick").primary().label("Pick the game file…").on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let f = f2.clone();
                            let _ = m2.update(cx, |this, cx| {
                                let pick = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("The game file it replaces".into()) });
                                let backend = this.backend.clone();
                                cx.spawn(async move |this, cx| {
                                    let Ok(Ok(Some(mut p))) = pick.await else { return };
                                    let Some(target) = p.pop().map(|t| t.to_string_lossy().to_string()) else { return };
                                    let r = backend.call("mod.addCopy", json!({ "path": f, "target": target })).await;
                                    let _ = this.update(cx, |this, cx| match r {
                                        Ok(_) => {
                                            this.choose.push(f.clone());
                                            let name = file_name(&target).to_ascii_lowercase();
                                            let _ = this.studio.update(cx, |s, _| {
                                                if let Some(p) = &mut s.project {
                                                    p.note_files([name]);
                                                }
                                            });
                                            this.toast(Tone::Success, format!("{} ships as a modified {}", file_name(&f), file_name(&target)), cx);
                                            this.refresh(cx);
                                        }
                                        Err(e) => this.toast(Tone::Error, e, cx),
                                    });
                                })
                                .detach();
                            });
                        })),
                )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_installed_name() {
        let list = json!({ "mods": [{ "id": "other", "name": "Other" }, { "id": "cassie-neon", "name": "Cassie Neon.tempest" }] });
        assert_eq!(installed_name(&list, "Cassie-Neon").as_deref(), Some("Cassie Neon.tempest"));
        assert_eq!(installed_name(&json!([{ "Id": "a", "Name": "A" }]), "a").as_deref(), Some("A"));
        assert_eq!(installed_name(&list, "missing"), None);
    }

    #[test]
    fn names_files_in_words() {
        assert_eq!(file_in_words("ChaosGame/CookedPCConsole/assembly.dat"), "Game data");
        assert_eq!(file_in_words("ChaosGame/CookedPCConsole/Lang_INT.dat"), "Texts (INT)");
        assert_eq!(file_in_words("ChaosGame/CookedPCConsole/CASSIE_ASSETS_SF.upk"), "CASSIE_ASSETS_SF");
    }
}
