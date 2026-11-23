//! The Mod view: a mod is the list of game files your changes touch. Each
//! one is listed with where its edits are (unsaved, saved in place, an
//! exported copy) and how it ships (a delta of only what changed, or the
//! whole file); its changes can be previewed. Details name the mod; Build
//! writes the mod folder (`mod.cook`), packs and signs it with tempest-cli
//! like the launcher, and Install puts it into a game folder.

use std::path::{Path, PathBuf};

use gpui::{div, prelude::*, px, AnyElement, App, Context, Entity, KeyDownEvent, PathPromptOptions, SharedString, Subscription, WeakEntity, Window};
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
    /// What changed is unfolded under the file's row.
    open: bool,
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
    movies: Vec<Entity<super::swf::MovieEditor>>,
    unapplied_particles: usize,
    applying_particles: bool,
    dir: String,
    targets: Vec<Target>,
    loading: bool,
    pub(super) meta: Meta,
    pub(super) busy: Option<String>,
    log: Vec<(Line, String)>,
    /// The package built last and whether it was signed.
    pub(super) built: Option<(String, bool)>,
    /// Whether `built` was packed with temporary `fast` (zstd) compression.
    pub(super) built_fast: bool,
    /// The id and version `built` was made with: Publish refuses a build older than the details.
    pub(super) built_as: Option<(String, String)>,
    /// The open game folder's detected patch version (e.g. `"8.1"`), when known.
    game_patch: Option<String>,
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
    /// The mod is installed in this game (`tempest mod list`): Remove from game shows.
    installed: bool,
    /// The last build's log is unfolded.
    show_log: bool,
    /// Whether the visual patch timeline is unfolded in `CompatMode::Range`.
    show_timeline: bool,
    /// How the game patches are picked, as on the website: every patch, a range, or some.
    compat_mode: CompatMode,
    /// A range's first click, waiting for its last.
    anchor: Option<String>,
    /// `mod-files/` and the icon as last read: the view renders often, the disk changes rarely.
    disk: std::cell::RefCell<Option<Disk>>,
    _subs: Vec<Subscription>,
}

/// What `problems`, `kind` and the file count need from disk, read at most every `Disk::TTL`.
struct Disk {
    at: std::time::Instant,
    files: std::rc::Rc<[PathBuf]>,
    icon: Option<(String, Option<String>)>,
}

impl Disk {
    const TTL: std::time::Duration = std::time::Duration::from_secs(2);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CompatMode {
    All,
    Range,
    Specific,
}

impl CompatMode {
    fn of(c: &modmeta::Compat) -> CompatMode {
        let first = modmeta::patches().first().map(|p| p.version.as_str());
        if !c.patches.is_empty() {
            CompatMode::Specific
        } else if c.from.is_some() && c.from.as_deref() == first && c.to.is_none() {
            CompatMode::All
        } else {
            CompatMode::Range
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// The game patches it works with (saved as `patches`, `patchFrom`, `patchTo`).
    pub compat: modmeta::Compat,
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
    ("link", "https://…"),
    ("summary", "One line that says what the mod does"),
    ("tags", "skin, maeve"),
    ("repository", "https://github.com/you/your-mod"),
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
            _ => &self.repository,
        }
    }

    pub fn text(&self, key: &str, cx: &App) -> String {
        self.input(key).read(cx).value().trim().to_string()
    }

    pub fn deps(&self, cx: &App) -> Vec<String> {
        self.deps.read(cx).value().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
    }
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

/// UE3 three-letter language code in plain words.
pub(super) fn lang_name(code: &str) -> Option<&'static str> {
    match code.to_ascii_uppercase().as_str() {
        "INT" => Some("English"),
        "FRA" => Some("French"),
        "DEU" => Some("German"),
        "ESN" | "ESM" => Some("Spanish"),
        "POL" => Some("Polish"),
        "RUS" => Some("Russian"),
        "POR" => Some("Portuguese"),
        "CHN" => Some("Chinese"),
        "JPN" => Some("Japanese"),
        "KOR" => Some("Korean"),
        "TUR" => Some("Turkish"),
        "THA" => Some("Thai"),
        "ITA" => Some("Italian"),
        _ => None,
    }
}

/// A game file as a player names it: `ChaosGame/CookedPCConsole/assembly.dat` → `Game data`.
fn file_in_words(rel: &str) -> String {
    let name = rel.rsplit(['/', '\\']).next().unwrap_or(rel);
    let lower = name.to_ascii_lowercase();
    if lower == "assembly.dat" {
        "Game data".into()
    } else if lower.starts_with("lang_") && lower.ends_with(".dat") {
        let code = &name[5..name.len() - 4];
        match lang_name(code) {
            Some(lang) => format!("{lang} texts ({name})"),
            None => format!("Texts ({name})"),
        }
    } else if lower.ends_with(".pck") {
        format!("Sounds: {}", &name[..name.len() - 4])
    } else {
        name.trim_end_matches(".upk").to_string()
    }
}

/// Read the per-game-folder draft metadata (`mod-<hash>.json`), or `Value::Null` when none is saved yet.
pub(crate) fn read_draft_meta(dir: &str) -> J {
    meta_file(dir).and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(J::Null)
}

fn detect_patch_in_path(dir: &str) -> Option<String> {
    let parts: Vec<&str> = dir.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    for part in parts.iter().rev() {
        if modmeta::known_patch(part) {
            return Some((*part).to_string());
        }
    }
    for part in parts.iter().rev() {
        for tok in part.split([' ', '-', '_', '(', ')']) {
            let tok = tok.strip_prefix('v').or_else(|| tok.strip_prefix('V')).unwrap_or(tok);
            if modmeta::known_patch(tok) {
                return Some(tok.to_string());
            }
        }
    }
    None
}

fn detect_game_patch(dir: &str) -> Option<String> {
    if let Some(root) = Path::new(dir).parent().and_then(|p| p.parent()) {
        let root_s = root.to_string_lossy();
        if let Some(v) = super::games::find()
            .into_iter()
            .find(|g| super::project::same_path(&g.path, &root_s))
            .and_then(|g| g.version)
            .filter(|v| modmeta::known_patch(v))
        {
            return Some(v);
        }
    }
    detect_patch_in_path(dir)
}

/// A file's change counts in words, from its `mod.preview` item.
fn counts(p: &J) -> Option<String> {
    if p.get("error").is_some() {
        return None;
    }
    Some(match p["mode"].as_str() {
        Some("unchanged") => "no change from the original: left out".to_string(),
        Some("whole") => format!("the whole file, {}", crate::model::human_size(p["bytes"].as_u64().unwrap_or(0))),
        _ => format!("{} changed, {} added, {} removed", p["changed"].as_u64().unwrap_or(0), p["added"].as_u64().unwrap_or(0), p["deleted"].as_u64().unwrap_or(0)),
    })
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

/// Pass a raw backend, OS or CLI error through `ui::explain_error` once (idempotent).
pub(super) fn explain(text: &str) -> String {
    let once = super::ui::explain_error(text);
    if let Some(added) = once.strip_prefix(text.trim_end_matches('.')) {
        let hint = added.trim_start_matches(['.', ' ']);
        if !hint.is_empty() && text.ends_with(hint) {
            return text.to_string();
        }
    }
    once
}

impl Cooker {
    /// Runs a Studio command once this Cooker is no longer being updated: commands read the Cooker back.
    fn run_studio(&self, cmd: super::Cmd, window: &mut Window, cx: &mut Context<Self>) {
        let studio = self.studio.clone();
        window.defer(cx, move |window, cx| {
            let _ = studio.update(cx, |s, cx| s.run(cmd, window, cx));
        });
    }

    pub fn new(backend: Backend, dir: String, project: Option<super::project::Project>, movies: Vec<Entity<super::swf::MovieEditor>>, studio: WeakEntity<Studio>, window: &mut Window, cx: &mut Context<Self>) -> Cooker {
        // A project keeps the details in its file; otherwise they are kept per game folder.
        let saved: J = match project.as_ref().map(|p| p.cooker.clone()).filter(|c| c.is_object()) {
            Some(c) => c,
            None => read_draft_meta(&dir),
        };
        let mut saved = saved;
        if let Some(p) = &project {
            for (k, v) in [("name", &p.name), ("id", &p.id), ("version", &p.version), ("author", &p.author)] {
                if saved[k].as_str().is_none_or(str::is_empty) {
                    saved[k] = json!(v);
                }
            }
        }
        let game_patch = detect_game_patch(&dir);
        let mut compat = modmeta::Compat::parse(saved["patches"].as_str().unwrap_or_default(), saved["patchFrom"].as_str().unwrap_or_default(), saved["patchTo"].as_str().unwrap_or_default());
        if !compat.is_set() {
            if let Some(ver) = &game_patch {
                compat = modmeta::Compat { patches: Vec::new(), from: Some(ver.clone()), to: None };
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
            compat,
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
        subs.push(cx.subscribe(&meta.api_key, |_, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
                cx.refresh_windows();
            }
        }));
        let extras = saved["extras"].as_array().into_iter().flatten().filter_map(|x| Some((x["src"].as_str()?.to_string(), x["dest"].as_str()?.to_string()))).collect();
        let dismissed = saved["dismissed"].as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.clone(), v.as_u64()?))).collect();
        let mut c = Cooker { backend, studio, movies, unapplied_particles: 0, applying_particles: false, dir, targets: Vec::new(), loading: true, meta, busy: None, log: Vec::new(), built: None, built_fast: false, built_as: None, game_patch, project, extras, dismissed, unmatched: None, choose: Vec::new(), account: None, seen: crate::backend::EDITS.load(std::sync::atomic::Ordering::SeqCst), tab: ModTab::Changes, installed: false, show_log: false, show_timeline: false, compat_mode: CompatMode::Range, anchor: None, disk: Default::default(), _subs: subs };
        c.compat_mode = CompatMode::of(&c.meta.compat);
        c.refresh(cx);
        c.load_account(cx);
        c.check_installed(cx);
        c
    }

    pub(crate) fn set_unapplied_particles(&mut self, count: usize, cx: &mut Context<Self>) {
        if self.unapplied_particles != count || (count == 0 && self.applying_particles) {
            self.unapplied_particles = count;
            if count == 0 {
                self.applying_particles = false;
            }
            cx.notify();
        }
    }

    pub(crate) fn sync_unapplied(&mut self, movies: Vec<Entity<super::swf::MovieEditor>>, unapplied_particles: usize, cx: &mut Context<Self>) {
        self.movies = movies;
        self.unapplied_particles = unapplied_particles;
        if unapplied_particles == 0 {
            self.applying_particles = false;
        }
        cx.notify();
    }

    /// `(unapplied_movie_count, total_unapplied_stage_edits, any_applying)`.
    fn unapplied_movies_status(&self, cx: &App) -> (usize, usize, bool) {
        let mut count = 0usize;
        let mut edits = 0usize;
        let mut applying = false;
        for m in &self.movies {
            let r = m.read(cx);
            let n = r.unapplied_count();
            if n > 0 {
                count += 1;
                edits += n;
                applying |= r.is_applying();
            }
        }
        (count, edits, applying)
    }

    fn unapplied_movie_names(&self, cx: &App) -> Vec<String> {
        let mut names: Vec<String> = self
            .movies
            .iter()
            .filter_map(|m| {
                let r = m.read(cx);
                r.has_unapplied().then(|| r.name.clone())
            })
            .collect();
        names.sort();
        names
    }

    fn unapplied_movie_problem(count: usize) -> String {
        if count == 1 {
            "1 UI movie has unapplied stage edits".into()
        } else {
            format!("{count} UI movies have unapplied stage edits")
        }
    }

    pub(crate) fn apply_unapplied_movies(&mut self, cx: &mut Context<Self>) {
        for m in self.movies.clone() {
            m.update(cx, |m, cx| m.apply(cx));
        }
        cx.notify();
    }

    fn unapplied_particle_problem(count: usize) -> String {
        if count <= 1 {
            "Unapplied particle curve edits".into()
        } else {
            format!("Unapplied particle curve edits ({count} curves)")
        }
    }

    pub(crate) fn apply_unapplied_particles(&mut self, cx: &mut Context<Self>) {
        if self.unapplied_particles == 0 || self.applying_particles {
            return;
        }
        self.applying_particles = true;
        let studio = self.studio.clone();
        cx.defer(move |cx| {
            let _ = studio.update(cx, |s, cx| s.apply_particle_drafts(cx));
        });
        cx.notify();
    }

    pub(super) fn toast(&self, tone: Tone, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        let _ = self.studio.update(cx, |s, cx| s.toast(tone, text, cx));
    }

    fn say(&mut self, line: Line, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        let text = if line == Line::Error { explain(&text) } else { text };
        self.log.push((line, text));
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
            "patches": self.meta.compat.patches.join(", "),
            "patchFrom": self.meta.compat.from.clone().unwrap_or_default(),
            "patchTo": self.meta.compat.to.clone().unwrap_or_default(),
        });
        for (k, _) in FIELDS {
            v[*k] = json!(self.meta.input(k).read(cx).value().to_string());
        }
        v
    }

    pub(super) fn draft_json(&self, cx: &App) -> J {
        self.meta_json(cx)
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
        let _ = self.studio.update(cx, |_, cx| cx.notify());
    }

    pub(super) fn mod_id(&self, cx: &App) -> String {
        let typed = self.meta.text("id", cx);
        if typed.is_empty() { super::project::id_of(&self.meta.text("name", cx)) } else { typed }
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
                    Err(e) => this.toast(Tone::Error, format!("Could not list the changes: {}", super::ui::explain_error(&e)), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_sources(&mut self, files: &J) {
        let old: Vec<(String, bool, bool, bool)> = self.targets.iter().map(|t| (t.target.clone(), t.included, t.whole, t.open)).collect();
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
                    open: false,
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
            if let Some((_, inc, whole, open)) = old.iter().find(|o| o.0 == t.target) {
                t.included = *inc;
                t.whole = *whole;
                t.open = *open;
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

    /// What changed: unfold the file's list (read first if it is not yet), or fold it.
    fn toggle_changes(&mut self, k: usize, cx: &mut Context<Self>) {
        let Some(t) = self.targets.get_mut(k) else { return };
        t.open = !t.open;
        if t.open && t.preview.is_none() {
            self.preview(k, cx);
        }
        cx.notify();
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
                            t.preview = Some(json!({ "error": super::ui::explain_error(&e) }));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// What the mod ships besides the game files: added files and the project's mod-files/.
    /// The cached read of `mod-files/` and the icon, refreshed when stale or the icon changed.
    fn disk(&self) -> std::cell::Ref<'_, Disk> {
        let icon = self.meta.icon.clone();
        let fresh = self.disk.borrow().as_ref().is_some_and(|d| d.at.elapsed() < Disk::TTL && d.icon.as_ref().map(|i| &i.0) == icon.as_ref());
        if !fresh {
            let files = self.project.as_ref().map(|p| walk(&p.mod_files())).unwrap_or_default().into();
            let icon = icon.map(|i| {
                let why = match std::fs::read(&i) {
                    Ok(data) => modmeta::icon_problem(&data),
                    Err(_) => Some(format!("The icon {} is not there any more", file_name(&i))),
                };
                (i, why)
            });
            *self.disk.borrow_mut() = Some(Disk { at: std::time::Instant::now(), files, icon });
        }
        std::cell::Ref::map(self.disk.borrow(), |d| d.as_ref().expect("filled above"))
    }

    /// Every file under the project's `mod-files/` (cached for `Disk::TTL`).
    fn shipped_files(&self) -> std::rc::Rc<[PathBuf]> {
        self.disk().files.clone()
    }

    /// Forget the cached disk read, after this view added or removed files.
    fn refresh_disk(&self) {
        self.disk.borrow_mut().take();
    }

    /// Why the mod's id (typed, or made from the name) is not one launchers accept.
    pub(super) fn id_problem(&self, cx: &App) -> Option<String> {
        let id = self.mod_id(cx);
        if modmeta::id_ok(&id) {
            None
        } else if self.meta.text("id", cx).is_empty() {
            Some(if id.is_empty() {
                "The name has no Latin letters or digits to make an id from: type one under Details ▸ Advanced, for example my-mod".into()
            } else {
                format!("The id made from the name (\"{id}\") is too short: type one under Details ▸ Advanced, at least 2 characters")
            })
        } else {
            Some(format!("The id \"{id}\" is not valid: 2 to 64 lower-case letters, digits, dots, dashes and underscores, starting with a letter or digit"))
        }
    }

    fn ships_code(&self) -> bool {
        let code = |p: &str| {
            let l = p.to_ascii_lowercase();
            l.ends_with(".dll") || (l.ends_with(".lua") && !l.ends_with(".d.lua"))
        };
        let mut files: Vec<String> = self.extras.iter().map(|e| e.0.clone()).collect();
        files.extend(self.shipped_files().iter().map(|f| f.to_string_lossy().to_string()));
        files.iter().any(|f| code(f))
    }

    fn ships_lua(&self) -> bool {
        let lua = |p: &str| p.to_ascii_lowercase().ends_with(".lua");
        self.extras.iter().any(|e| lua(&e.0)) || self.project.as_ref().is_some_and(|p| {
            let scripts = p.mod_files().join("scripts");
            self.shipped_files().iter().any(|f| f.starts_with(&scripts) && lua(&f.to_string_lossy()))
        })
    }

    /// Why Build cannot run yet (the launcher's and the repository's checks).
    pub(super) fn problems(&self, cx: &App) -> Vec<String> {
        let mut p = Vec::new();
        let (unapplied_movies, _, _) = self.unapplied_movies_status(cx);
        if unapplied_movies > 0 {
            p.push(Self::unapplied_movie_problem(unapplied_movies));
        }
        if self.unapplied_particles > 0 {
            p.push(Self::unapplied_particle_problem(self.unapplied_particles));
        }
        let has_name = !self.meta.text("name", cx).is_empty();
        if !has_name {
            p.push("Give the mod a name".into());
        } else if self.meta.text("name", cx).chars().count() > modmeta::MAX_NAME {
            p.push(format!("The name is longer than {} characters", modmeta::MAX_NAME));
        }
        // The id comes from the name: its rule only matters once there is one.
        if has_name {
            p.extend(self.id_problem(cx));
        }
        let v = self.meta.text("version", cx);
        if !v.is_empty() && !modmeta::is_semver(&v) {
            p.push(format!("The version is three numbers like 1.0.0, optionally -beta.1 (\"{v}\" is not)"));
        }
        let id = self.mod_id(cx);
        let mut needed = std::collections::HashSet::new();
        for d in self.meta.deps(cx) {
            if let Some(why) = modmeta::dependency_problem(&d) {
                p.push(format!("Dependency: {why}"));
            } else if let Some(dep) = modmeta::dependency_id(&d) {
                if dep == id {
                    p.push(format!("Dependency: the mod lists itself ({id}) among the mods it needs"));
                } else if !needed.insert(dep.clone()) {
                    p.push(format!("Dependency: {dep} is listed twice"));
                }
            }
        }
        if let Some((_, Some(why))) = &self.disk().icon {
            p.push(why.clone());
        }
        if self.meta.text("summary", cx).chars().count() > modmeta::MAX_SUMMARY {
            p.push(format!("The summary is longer than {} characters", modmeta::MAX_SUMMARY));
        }
        let project_files = !self.shipped_files().is_empty();
        if !self.targets.iter().any(|t| t.included) && self.extras.is_empty() && !project_files && unapplied_movies == 0 && self.unapplied_particles == 0 {
            p.push("Nothing to ship: edit a package or a table first, or add files".into());
        }
        if self.ships_code() && !signing_key().exists() {
            p.push("The mod ships code (Lua or DLLs): create your signing key under Signing and publishing, or the launcher refuses it".into());
        }
        p
    }

    /// Build, asking first when the project already has a package of this version.
    fn build(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.build_with(false, false, window, cx);
    }

    fn build_with(&mut self, install_after: bool, fast_override: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (id, version) = (self.mod_id(cx), self.version(cx));
        if let Some(out) = self.project.as_ref().map(|p| p.builds().join(format!("{id}-{version}.tempest"))).filter(|o| o.exists()) {
            let me = cx.entity().downgrade();
            super::dialogs::confirm(window, cx, format!("Replace the build of {version}?"), format!("{} already exists. Build again over it, or change the version first (a version already published cannot be uploaded again).", file_name(&out.to_string_lossy())), "Build again", false, move |_, cx| {
                let _ = me.update(cx, |this, cx| this.build_inner(install_after, fast_override, cx));
            });
            return;
        }
        self.build_inner(install_after, fast_override, cx);
    }

    pub(super) fn build_now(&mut self, cx: &mut Context<Self>) {
        self.build_inner(false, false, cx);
    }

    fn cook_request(&self, staging: &Path, compression: &str, cx: &App) -> (usize, J) {
        let (id, version) = (self.mod_id(cx), self.version(cx));
        let mut extras: Vec<J> = self.extras.iter().map(|(s, d)| json!({ "src": s, "dest": d })).collect();
        extras.extend(self.project.as_ref().map(|p| p.mod_files()).filter(|m| m.is_dir()).map(|m| json!({ "src": m.to_string_lossy(), "dest": "" })));
        let name = self.meta.text("name", cx);
        let author = self.meta.text("author", cx);
        let link = self.meta.text("link", cx);
        let readme = self.meta.readme.read(cx).value().to_string();
        let icon = self.meta.icon.clone();
        let items: Vec<J> = self.targets.iter().filter(|t| t.included).map(Self::item).collect();
        let count = items.len();
        let mut deps = self.meta.deps(cx);
        // Lua needs the scripting runtime.
        if self.ships_lua() && !deps.iter().any(|d| d.to_ascii_lowercase().starts_with("tempest-scripting")) {
            deps.push("tempest-scripting>=1.0.0".into());
        }
        let authors = if author.is_empty() { json!([]) } else { json!([{ "name": author, "link": link }]) };
        (count, json!({ "dir": staging.to_string_lossy(), "id": id, "name": name, "version": version, "authors": authors, "readme": readme, "icon": icon, "dependencies": deps, "items": items, "compression": compression, "extras": extras }))
    }

    fn build_inner(&mut self, install_after: bool, fast_override: bool, cx: &mut Context<Self>) {
        self.refresh_disk();
        if !self.problems(cx).is_empty() {
            return;
        }
        let (id, version) = (self.mod_id(cx), self.version(cx));
        let docs = std::env::var_os("HOME").map(|h| Path::new(&h).join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        // A project builds into its builds/ folder; Build & install of an unnamed
        // mod into a temporary one; otherwise ask where.
        let fixed = self.project.as_ref().map(|p| p.builds().join(format!("{id}-{version}.tempest"))).or_else(|| install_after.then(|| std::env::temp_dir().join("tempest-studio-native").join("builds").join(format!("{id}-{version}.tempest"))));
        let target = fixed.is_none().then(|| cx.prompt_for_new_path(&docs, Some(&format!("{id}-{version}.tempest"))));
        let use_fast = self.meta.fast || fast_override;
        let compression = if use_fast { "fast" } else { "small" };
        let staging = std::env::temp_dir().join("tempest-studio-native").join("staging").join(&id);
        let (item_count, req) = self.cook_request(&staging, compression, cx);
        let made_as = (id.clone(), version.clone());
        cx.spawn(async move |this, cx| {
            let out = match (fixed, target) {
                (Some(f), _) => {
                    let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
                    f
                }
                (None, Some(t)) => {
                    let Ok(Ok(Some(o))) = t.await else { return };
                    o
                }
                _ => return,
            };
            let out = out.to_string_lossy().to_string();
            let _ = std::fs::remove_dir_all(&staging);
            let _ = std::fs::create_dir_all(&staging);
            let Ok(call) = this.update(cx, |this, cx| {
                this.busy = Some("Building…".into());
                this.log.clear();
                this.built = None;
                this.built_fast = false;
                this.say(Line::Info, format!("Building {item_count} file{}…", if item_count == 1 { "" } else { "s" }), cx);
                this.backend.call("mod.cook", req)
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
                    this.say(Line::Error, format!("Build failed: {}", super::ui::explain_error(&e)), cx);
                    this.busy = None;
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
                match packed {
                    Ok((0, _)) => {}
                    Ok((_, text)) => {
                        this.say(Line::Error, format!("tempest mod pack failed: {}", super::ui::explain_error(text.trim())), cx);
                        return;
                    }
                    Err(e) => {
                        this.say(Line::Error, super::ui::explain_error(&e), cx);
                        return;
                    }
                }
                match signed {
                    Some(Ok((0, text))) => this.say(Line::Ok, format!("Signed{}", text.lines().find_map(|l| l.split_once(" with ").map(|(_, w)| format!(" with {}", w.trim()))).unwrap_or_default()), cx),
                    Some(Ok((_, text))) => this.say(Line::Error, format!("Signing failed: {}", super::ui::explain_error(text.trim())), cx),
                    Some(Err(e)) => this.say(Line::Error, super::ui::explain_error(&e), cx),
                    None => this.say(Line::Info, "No signing key: the package is unsigned (fine for assets, refused for code)", cx),
                }
                this.say(Line::Ok, format!("Built {}", out), cx);
                this.built = Some((out.clone(), signed_ok));
                this.built_fast = use_fast;
                this.built_as = Some(made_as);
                if install_after {
                    this.install_here(cx);
                } else {
                    this.toast(Tone::Success, format!("Mod built: {}", file_name(&out)), cx);
                }
            });
        })
        .detach();
    }

    /// Repack `out` with `"small"` (LZMA) release compression before uploading, when `built` came from `Build & install`.
    pub(super) fn build_release_into(&mut self, out: String, cx: &mut Context<Self>, after: impl FnOnce(&mut Cooker, Result<(), String>, &mut Context<Cooker>) + 'static) {
        let id = self.mod_id(cx);
        let made_as = (id.clone(), self.version(cx));
        let staging = std::env::temp_dir().join("tempest-studio-native").join("staging").join(&id);
        let _ = std::fs::remove_dir_all(&staging);
        let _ = std::fs::create_dir_all(&staging);
        if let Some(parent) = Path::new(&out).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let (item_count, req) = self.cook_request(&staging, "small", cx);
        self.busy = Some("Packing smallest file…".into());
        self.say(Line::Info, format!("Repacking {item_count} file{} for release…", if item_count == 1 { "" } else { "s" }), cx);
        let call = self.backend.call("mod.cook", req);
        cx.spawn(async move |this, cx| {
            if let Err(e) = call.await {
                let _ = this.update(cx, |this, cx| {
                    this.busy = None;
                    let msg = format!("Build failed: {}", super::ui::explain_error(&e));
                    this.say(Line::Error, msg.clone(), cx);
                    after(this, Err(msg), cx);
                });
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
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match packed {
                    Ok((0, _)) => {}
                    Ok((_, text)) => {
                        let msg = format!("tempest mod pack failed: {}", super::ui::explain_error(text.trim()));
                        this.say(Line::Error, msg.clone(), cx);
                        after(this, Err(msg), cx);
                        return;
                    }
                    Err(e) => {
                        let msg = super::ui::explain_error(&e);
                        this.say(Line::Error, msg.clone(), cx);
                        after(this, Err(msg), cx);
                        return;
                    }
                }
                let signed_ok = match signed {
                    Some(Ok((0, _))) => true,
                    Some(Ok((_, text))) => {
                        let msg = format!("Signing failed: {}", super::ui::explain_error(text.trim()));
                        this.say(Line::Error, msg.clone(), cx);
                        after(this, Err(msg), cx);
                        return;
                    }
                    Some(Err(e)) => {
                        let msg = super::ui::explain_error(&e);
                        this.say(Line::Error, msg.clone(), cx);
                        after(this, Err(msg), cx);
                        return;
                    }
                    None => false,
                };
                this.say(Line::Ok, format!("Repacked {} (smallest file)", out), cx);
                this.built = Some((out, signed_ok));
                this.built_fast = false;
                this.built_as = Some(made_as);
                after(this, Ok(()), cx);
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

    /// Build & install: a fast build, then into this game (leaving the saved compression choice untouched).
    pub(crate) fn build_and_install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.build_with(true, true, window, cx);
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
                    Ok(None) => {
                        this.installed = false;
                        this.toast(Tone::Info, "This mod is not installed in this game", cx);
                    }
                    Ok(Some((0, _, name))) => {
                        this.installed = false;
                        this.say(Line::Ok, format!("Removed {name} from {game}"), cx);
                        this.toast(Tone::Success, "Mod removed from the game", cx);
                    }
                    Ok(Some((_, text, _))) => this.say(Line::Error, format!("Could not remove the mod: {}", super::ui::explain_error(text.trim())), cx),
                    Err(e) => this.say(Line::Error, super::ui::explain_error(&e), cx),
                }
            });
        })
        .detach();
    }

    /// Whether this mod is installed in this game, so Remove from game shows only then.
    fn check_installed(&mut self, cx: &mut Context<Self>) {
        let (game, id) = (self.game_root(), self.mod_id(cx));
        if id.is_empty() || cli_path().is_none() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { run_cli_split(&["list".into(), game, "--json".into()]).ok().and_then(|(_, out, _)| serde_json::from_str::<J>(out.trim()).ok()).is_some_and(|l| installed_name(&l, &id).is_some()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.installed = found;
                cx.notify();
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
                        this.installed |= game == this.game_root();
                        this.say(Line::Ok, format!("Installed into {game}. Play it from Tempest Launcher to test it."), cx);
                        this.toast(Tone::Success, "Mod installed: play from Tempest Launcher to test it", cx);
                    }
                    Ok((_, text)) => this.say(Line::Error, format!("Install failed: {}", super::ui::explain_error(text.trim())), cx),
                    Err(e) => this.say(Line::Error, super::ui::explain_error(&e), cx),
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
            "apply-movies" => self.apply_unapplied_movies(cx),
            "apply-particles" => self.apply_unapplied_particles(cx),
            "tab" => {
                self.tab = match rest.trim() {
                    "details" => ModTab::Details,
                    "publish" => ModTab::Publish,
                    _ => ModTab::Changes,
                }
            }
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
        let (unapplied_movies, unapplied_edits, applying_movies) = self.unapplied_movies_status(cx);
        let unapplied_banner = (unapplied_movies > 0).then(|| {
            let names = self.unapplied_movie_names(cx);
            let detail = format!(
                "{}: {unapplied_edits} stage edit{} staged in the movie editor. Apply now so {} included in your mod before building.",
                names.join(", "),
                if unapplied_edits == 1 { "" } else { "s" },
                if unapplied_edits == 1 { "it is" } else { "they are" }
            );
            h_flex()
                .p_3()
                .gap_3()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(theme.warning)
                .bg(theme.warning.opacity(0.12))
                .child(gpui_component::Icon::new(gpui_kit_assets::IconName::TriangleAlert).small().text_color(theme.warning))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(Self::unapplied_movie_problem(unapplied_movies)))
                        .child(div().text_xs().text_color(theme.muted_foreground).whitespace_normal().child(detail)),
                )
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .child(
                            Button::new("cook-files-apply-movies")
                                .small()
                                .label(if applying_movies { "Applying…" } else { "Apply now" })
                                .disabled(applying_movies)
                                .tooltip(if applying_movies { "Applying pending UI movie stage edits…" } else { "Write pending UI movie stage edits into the package so they ship in your mod" })
                                .on_click(cx.listener(|this, _, _, cx| this.apply_unapplied_movies(cx))),
                        )
                        .child(crate::driver::probe("cook-files-apply-movies")),
                )
        });
        let unapplied_particles = self.unapplied_particles;
        let applying_particles = self.applying_particles;
        let particle_banner = (unapplied_particles > 0).then(|| {
            let detail = format!(
                "{unapplied_particles} curve{} edited in the particle preview. Apply now so {} included in your mod before building.",
                if unapplied_particles == 1 { "" } else { "s" },
                if unapplied_particles == 1 { "it is" } else { "they are" }
            );
            h_flex()
                .p_3()
                .gap_3()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(theme.warning)
                .bg(theme.warning.opacity(0.12))
                .child(gpui_component::Icon::new(gpui_kit_assets::IconName::TriangleAlert).small().text_color(theme.warning))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(Self::unapplied_particle_problem(unapplied_particles)))
                        .child(div().text_xs().text_color(theme.muted_foreground).whitespace_normal().child(detail)),
                )
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .child(
                            Button::new("cook-files-apply-particles")
                                .small()
                                .label(if applying_particles { "Applying…" } else { "Apply now" })
                                .disabled(applying_particles)
                                .tooltip(if applying_particles { "Applying pending particle curve edits…" } else { "Write pending particle curve edits into the package so they ship in your mod" })
                                .on_click(cx.listener(|this, _, _, cx| this.apply_unapplied_particles(cx))),
                        )
                        .child(crate::driver::probe("cook-files-apply-particles")),
                )
        });
        if self.targets.is_empty() {
            if unapplied_banner.is_some() || particle_banner.is_some() {
                return v_flex()
                    .gap_2()
                    .children(unapplied_banner)
                    .children(particle_banner)
                    .when(self.loading, |d| d.child(div().h(px(64.0)).child(crate::studio::ui::loading("Looking for your changes…", &theme))))
                    .into_any_element();
            }
            if self.loading {
                return div().h(px(120.0)).child(crate::studio::ui::loading("Looking for your changes…", &theme)).into_any_element();
            }
            let go = |id: &'static str, icon: gpui_kit_assets::IconName, label: &'static str, cmd: super::Cmd, cx: &mut Context<Self>| {
                Button::new(id).small().icon(gpui_component::Icon::new(icon)).label(label).on_click(cx.listener(move |this, _, window, cx| {
                    this.run_studio(cmd, window, cx);
                }))
            };
            return crate::studio::ui::empty(gpui_kit_assets::IconName::Pencil, "No changes yet", "Change a champion's look, a number in the game data, a sound or a map: every game file you change shows up here and goes into your mod.", &theme)
                .h_auto()
                .py_6()
                .child(h_flex().gap_2().flex_wrap().justify_center().child(go("cook-go-champions", gpui_kit_assets::IconName::Users, "Champions", super::Cmd::ShowChampions, cx)).child(go("cook-go-data", gpui_kit_assets::IconName::Database, "Game data", super::Cmd::ShowAssembly, cx)).child(go("cook-go-sounds", gpui_kit_assets::IconName::Volume2, "Sounds", super::Cmd::ShowAudio, cx)))
                .into_any_element();
        }
        let mut list = v_flex()
            .when_some(unapplied_banner, |d, b| d.child(div().pb_2().child(b)))
            .when_some(particle_banner, |d, b| d.child(div().pb_2().child(b)));
        if self.targets.len() > 1 {
            list = list.child(self.render_bulk(cx));
        }
        for (k, t) in self.targets.iter().enumerate() {
            let (_, source, origin) = &t.sources[t.chosen];
            let origin = match origin.as_str() {
                "unsaved" => "Changed in the Studio".to_string(),
                // A "Save as" of the game file, kept elsewhere: say where.
                "copy" => format!("Your copy at {source}"),
                _ => "Saved in the game folder".to_string(),
            };
            let ships = if t.script { "" } else if t.whole { " · whole file" } else { " · only the changes" };
            // The text column starts after the checkbox (16) and the gap (12): what unfolds under a row lines up with it.
            let indent = px(28.0);
            let mut row = v_flex()
                .py_2()
                .gap_1()
                .when(k > 0, |d| d.border_t_1().border_color(theme.border))
                .child(
                    h_flex()
                        .gap_3()
                        .child(div().relative().child(Checkbox::new(("inc", k)).checked(t.included).tooltip(if t.included { "In the mod: click to leave it out" } else { "Left out: click to put it in" }).on_click(cx.listener(move |this, v: &bool, _, cx| {
                            if let Some(t) = this.targets.get_mut(k) {
                                t.included = *v;
                            }
                            cx.notify();
                        }))).child(crate::driver::probe(format!("cook-inc-{k}"))))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_sm()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(if t.included { theme.foreground } else { theme.muted_foreground })
                                                .child(file_in_words(&t.rel)),
                                        )
                                        .when(!t.included, |d| d.child(crate::studio::ui::badge(None, "left out", theme.muted_foreground, &theme)))
                                        .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().font_family(crate::studio::ui::mono()).text_color(theme.muted_foreground).child(t.rel.clone())),
                                )
                                .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_xs().text_color(theme.muted_foreground).child(match t.preview.as_ref().and_then(counts) {
                                    Some(c) => format!("{origin}{ships} · {c}"),
                                    None => format!("{origin}{ships}"),
                                })),
                        )
                        .child(Button::new(("prev", k)).xsmall().ghost().icon(gpui_component::Icon::new(if t.open { gpui_kit_assets::IconName::ChevronDown } else { gpui_kit_assets::IconName::ChevronRight })).label(if t.open { "Hide changes" } else { "What changed" }).on_click(cx.listener(move |this, _, _, cx| this.toggle_changes(k, cx))))
                        .child({
                            let me = cx.entity();
                            div().relative().child(Button::new(("more", k)).xsmall().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Ellipsis)).tooltip("More: ship the whole file, discard, remove from the list").dropdown_menu(move |menu, _, cx| me.update(cx, |c, cx| c.target_menu(k, menu, cx)))).child(crate::driver::probe(format!("more-{k}")))
                        }),
                );
            if t.sources.len() > 1 {
                let mut pick = h_flex().pl(indent).gap_1().flex_wrap().child(div().text_xs().text_color(theme.muted_foreground).child("Ship"));
                for (j, s) in t.sources.iter().enumerate() {
                    let label = match s.2.as_str() {
                        "unsaved" => "the changes in the Studio".to_string(),
                        "copy" => format!("the copy {}", file_name(&s.1)),
                        _ => "the saved file".to_string(),
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
                row = row.child(pick);
            }
            // Unfolded by What changed only: a list per file made the page long.
            if let Some(e) = t.preview.as_ref().and_then(|p| p["error"].as_str()) {
                row = row.child(div().pl(indent).text_xs().text_color(theme.danger).whitespace_normal().child(explain(e)));
            }
            if t.open {
                let mut changes = v_flex().ml(indent).mt_1().p_2().gap_0p5().rounded_md().bg(theme.background);
                match &t.preview {
                    None => changes = changes.child(div().text_xs().text_color(theme.muted_foreground).child("Reading what changed…")),
                    Some(p) => {
                        changes = changes.child(div().text_xs().text_color(theme.muted_foreground).child(format!("{}, {}", counts(p).unwrap_or_default(), t.base)));
                        let all = p["changes"].as_array().cloned().unwrap_or_default();
                        for c in all.iter().take(60) {
                            changes = changes.child(h_flex().gap_2().text_xs().child(div().flex_none().w(px(60.0)).text_color(theme.muted_foreground).child(c["op"].as_str().unwrap_or_default().to_string())).child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(c["what"].as_str().unwrap_or_default().to_string())));
                        }
                        if all.len() > 60 {
                            changes = changes.child(div().text_xs().text_color(theme.muted_foreground).child(format!("and {} more", all.len() - 60)));
                        }
                        for w in p["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()) {
                            changes = changes.child(div().text_xs().text_color(theme.warning).child(w.to_string()));
                        }
                    }
                }
                row = row.child(changes);
            }
            list = list.child(row);
        }
        list.into_any_element()
    }
}

impl Cooker {
    fn render_icon(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (id, name) = (self.mod_id(cx), self.meta.text("name", cx));
        let has = self.meta.icon.is_some();
        h_flex()
            .gap_3()
            .child(crate::studio::ui::mod_icon(self.meta.icon.as_deref(), &id, &name, 64.0, &theme))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(match &self.meta.icon {
                        Some(icon) => file_name(icon),
                        None => "No icon: the mod list shows its initials. A square PNG, 1 MB at most.".into(),
                    }))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(Button::new("icon-pick").xsmall().label(if has { "Change…" } else { "Choose an icon…" }).on_click(cx.listener(|this, _, _, cx| this.pick_icon(cx))))
                            .when(has, |d| {
                                d.child(Button::new("icon-remove").xsmall().ghost().label("Remove").on_click(cx.listener(|this, _, _, cx| {
                                    this.meta.icon = None;
                                    this.save_meta(cx);
                                    cx.notify();
                                })))
                            }),
                    ),
            )
            .into_any_element()
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

    /// Other files the mod ships as they are: what was added, then the project's mod-files/.
    fn render_extras(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut list = v_flex().gap_1();
        for (k, (src, dest)) in self.extras.iter().enumerate() {
            list = list.child(
                h_flex()
                    .gap_2()
                    .child(div().flex_none().max_w(px(260.0)).overflow_hidden().text_ellipsis().whitespace_nowrap().font_family(crate::studio::ui::mono()).text_xs().child(dest.clone()))
                    .child(div().flex_1().min_w_0().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(format!("from {src}")))
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
            .gap_2()
            .when(self.extras.is_empty() && self.project.is_none(), |d| d.child(crate::studio::ui::hint("Game packages (.upk), data tables (.dat), sound banks (.pck), Lua scripts, or DLLs edited in another tool. Game packages, data tables and sound banks you add ship only their differences.", &theme)))
            .child(list)
            .child(self.render_project_files(cx))
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
                Err(e) => this.toast(Tone::Error, super::ui::explain_error(&e), cx),
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
            .item(item(if t.open { "Hide the changes" } else { "Show the changes" }.into(), Box::new(move |this, _, cx| this.toggle_changes(k, cx))))
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
        // Copies are forgotten above; unsaved edits are discarded (until then the file stays listed); what is left is a file saved in place.
        if !self.removable(t) {
            menu
        } else if self.project.is_some() {
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
                                    this.toast(Tone::Error, super::ui::explain_error(&e), cx);
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
        self.dismiss_all(vec![k], cx);
    }

    fn dismiss_all(&mut self, mut ks: Vec<usize>, cx: &mut Context<Self>) {
        ks.retain(|&k| k < self.targets.len());
        ks.sort_unstable();
        ks.dedup();
        let mut names = Vec::new();
        for k in ks.into_iter().rev() {
            let t = self.targets.remove(k);
            if let Some(m) = t.saved_modified {
                self.dismissed.insert(t.target.clone(), m);
            }
            names.push(file_name(&t.target).to_ascii_lowercase());
        }
        let _ = self.studio.update(cx, |s, _| {
            if let Some(p) = &mut s.project {
                p.files.retain(|f| !names.contains(f));
            }
        });
        self.save_meta(cx);
        cx.notify();
    }

    /// Whether a file can leave the list: a project's file, or an in-place
    /// save. A file with unsaved Studio edits is always listed (`refresh`):
    /// it is saved or its edits are discarded first.
    fn removable(&self, t: &Target) -> bool {
        !t.sources.iter().any(|s| s.2 == "unsaved") && (self.project.is_some() || t.saved_modified.is_some())
    }

    /// The files left out (unticked) that can leave the list.
    fn left_out(&self) -> Vec<usize> {
        self.targets.iter().enumerate().filter(|(_, t)| !t.included && self.removable(t)).map(|(k, _)| k).collect()
    }

    /// Take every unticked file off the list at once, after asking.
    fn remove_left_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ks = self.left_out();
        if ks.is_empty() {
            return;
        }
        let n = ks.len();
        let files = if n == 1 { "1 file".to_string() } else { format!("{n} files") };
        let (title, body) = if self.project.is_some() {
            (format!("Remove {files} from the project?"), "They leave this mod's list of files. Nothing is deleted from disk, and a file comes back when you change it again.")
        } else {
            (format!("Remove {files} from the list?"), "They stay hidden here until you save them again. Nothing is deleted from disk.")
        };
        let me = cx.entity().downgrade();
        super::dialogs::confirm(window, cx, title, body, "Remove", false, move |_, cx| {
            let ks = ks.clone();
            let _ = me.update(cx, |this, cx| this.dismiss_all(ks, cx));
        });
    }

    /// Above the files: tick or untick them all, and remove the unticked ones.
    fn render_bulk(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let total = self.targets.len();
        let included = self.targets.iter().filter(|t| t.included).count();
        let all = included == total;
        let left_out = self.left_out().len();
        let unticked = total - included;
        let remove_tip = if left_out > 0 {
            "Take the unticked files off this list"
        } else if unticked > 0 {
            "Files with unsaved edits stay listed: save them, or discard their edits from the ⋯ menu"
        } else {
            "Untick the files you want to remove first"
        };
        h_flex()
            .gap_3()
            .pb_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .relative()
                    .child(
                        Checkbox::new("inc-all")
                            .checked(all)
                            .label(format!("{included} of {total} files in the mod"))
                            .tooltip(if all { "Leave every file out of the mod" } else { "Put every file in the mod" })
                            .on_click(cx.listener(|this, v: &bool, _, cx| {
                                for t in &mut this.targets {
                                    t.included = *v;
                                }
                                cx.notify();
                            })),
                    )
                    .child(crate::driver::probe("cook-inc-all")),
            )
            .child(div().flex_1())
            .child(
                div()
                    .relative()
                    .child(
                        Button::new("cook-remove-left-out")
                            .xsmall()
                            .ghost()
                            .icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Trash))
                            .label(if left_out > 0 { format!("Remove {left_out} unticked") } else { "Remove unticked".to_string() })
                            .tooltip(remove_tip)
                            .disabled(left_out == 0)
                            .on_click(cx.listener(|this, _, window, cx| this.remove_left_out(window, cx))),
                    )
                    .child(crate::driver::probe("cook-remove-left-out")),
            )
            .into_any_element()
    }

    /// The project's mod-files/ (shipped as laid out).
    fn render_project_files(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(p) = &self.project else { return div().into_any_element() };
        let files = p.mod_files();
        let n = self.shipped_files().len();
        h_flex()
            .gap_2()
            .child(gpui_component::Icon::new(gpui_kit_assets::IconName::Folder).small().text_color(theme.muted_foreground))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().child(format!("mod-files/: {n} file{}", if n == 1 { "" } else { "s" })))
                    .child(crate::studio::ui::hint("Shipped as laid out: scripts/ (Lua), dlls/64/ and dlls/32/, files/<game path>.", &theme)),
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

/// Which tab fixes a problem `problems` words: the place to go from the checklist.
fn problem_tab(p: &str) -> Option<ModTab> {
    let l = p.to_ascii_lowercase();
    if l.starts_with("nothing to ship") || l.contains("unapplied stage edits") || l.contains("unapplied particle curve edits") {
        None
    } else if l.contains("summary") || l.contains("signing key") {
        Some(ModTab::Publish)
    } else {
        Some(ModTab::Details)
    }
}

impl Cooker {
    /// The mod's kind as the website shows it: assets, scripts (Lua) or native (DLLs).
    fn kind(&self) -> (&'static str, gpui_kit_assets::IconName) {
        let mut files: Vec<String> = self.extras.iter().map(|e| e.0.to_ascii_lowercase()).collect();
        files.extend(self.shipped_files().iter().map(|f| f.to_string_lossy().to_ascii_lowercase()));
        if files.iter().any(|f| f.ends_with(".dll")) {
            ("Native", gpui_kit_assets::IconName::SquareTerminal)
        } else if files.iter().any(|f| f.ends_with(".lua") && !f.ends_with(".d.lua")) {
            ("Scripts", gpui_kit_assets::IconName::FileCode)
        } else {
            ("Assets", gpui_kit_assets::IconName::Box)
        }
    }

    /// The header: the mod (icon, name, facts), its actions, and the tabs under it.
    fn render_header(&mut self, title: String, problems: &[String], cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let cli = cli_path().is_some();
        let busy = self.busy.clone();
        let blocked = !problems.is_empty() || busy.is_some() || !cli;
        let can_use = self.built.is_some() && busy.is_none();
        let unnamed = self.project.is_none() && self.meta.text("name", cx).is_empty();
        let id = self.mod_id(cx);
        let author = self.meta.text("author", cx);
        let included = self.targets.iter().filter(|t| t.included).count() + self.extras.len();
        let mut facts: Vec<String> = Vec::new();
        if !author.is_empty() {
            facts.push(format!("by {author}"));
        }
        facts.push(self.kind().0.to_string());
        facts.push(format!("v{}", self.version(cx)));
        facts.push(format!("{included} file{} in the mod", if included == 1 { "" } else { "s" }));
        if self.installed {
            facts.push("installed in this game".into());
        }
        let me = cx.entity().downgrade();
        let removable = !id.is_empty() && cli;
        let remove_tip = match &busy {
            Some(b) => b.clone(),
            None if !cli => "The mod packer (tempest-cli) is missing from this Studio".into(),
            None => "Take this mod out of the game folder that is open".into(),
        };
        let actions = h_flex()
            .flex_wrap()
            .justify_end()
            .gap_2()
            // An unnamed mod's first step: its name (and its own folder).
            .when(unnamed, |d| {
                d.child(Button::new("cook-name").small().primary().label("Name this mod…").tooltip("Gives the mod its name and its own folder: it saves itself as you work").on_click(cx.listener(|this, _, window, cx| {
                    this.run_studio(super::Cmd::NewProject, window, cx);
                })))
            })
            .when(self.installed, |d| d.child(Button::new("cook-remove").small().ghost().label("Remove from game").tooltip(remove_tip).disabled(busy.is_some() || !cli).on_click(cx.listener(|this, _, _, cx| this.remove_from_game(cx)))))
            .child(Button::new("cook-play").small().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Gamepad2)).label("Play in Tempest Launcher").tooltip("Tempest Launcher starts the game with your installed mods").on_click(|_, _, cx| cx.open_url("tempest://")))
            .child(
                Button::new("cook-more").small().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Ellipsis)).tooltip("Build only, install elsewhere, save a copy, publish").dropdown_menu(move |menu, _, _| {
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
                        .item(item("Remove from game", removable, |this, _, cx| this.remove_from_game(cx)))
                        .separator()
                        .item(item("Publish…", can_use, |this, w, cx| this.publish_dialog(w, cx)))
                        .item(item("Upgrade another mod to deltas…", true, |this, w, cx| {
                            this.run_studio(super::Cmd::UpgradeMod, w, cx);
                        }))
                }),
            )
            .child(
                div().relative().child(
                    Button::new("cook-build")
                        .small()
                        .when(!unnamed, |b| b.primary())
                        .icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Hammer))
                        .label(busy.clone().unwrap_or_else(|| "Build & install".into()))
                        .tooltip(match problems.first() {
                            Some(p) => format!("Before building: {p}"),
                            None if !cli => "The mod packer (tempest-cli) is missing from this Studio".into(),
                            None => "Build the mod (fast) and install it into this game, ready to play from Tempest Launcher".into(),
                        })
                        .disabled(blocked)
                        .on_click(cx.listener(|this, _, window, cx| this.build_and_install(window, cx))),
                ).child(crate::driver::probe("cook-build")),
            );
        let tab = self.tab;
        let changes = self.targets.len() + self.extras.len();
        let tab_item = |id: &'static str, label: &'static str, count: Option<usize>, t: ModTab, cx: &mut Context<Self>| {
            let on = tab == t;
            h_flex()
                .id(id)
                .relative()
                .min_h(px(36.0))
                .py_1()
                .px_1p5()
                .gap_1p5()
                .text_sm()
                .border_b_2()
                .border_color(if on { theme.primary } else { gpui::transparent_black() })
                .text_color(if on { theme.foreground } else { theme.muted_foreground })
                .when(on, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                .cursor_pointer()
                .focusable()
                .tab_index(0)
                .hover(|s| s.text_color(theme.foreground))
                .focus_visible(|s| s.rounded_t_sm().border_1().border_color(theme.ring).text_color(theme.foreground))
                .child(label)
                .when_some(count.filter(|n| *n > 0), |d, n| d.child(div().px_1p5().rounded_full().bg(theme.secondary).text_xs().text_color(theme.muted_foreground).child(n.to_string())))
                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                    if matches!(e.keystroke.key.as_str(), "enter" | "space") && !e.keystroke.modifiers.modified() {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.tab = t;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tab = t;
                    cx.notify();
                }))
                .child(crate::driver::probe(id))
        };
        let tabs = h_flex().gap_3().flex_wrap().child(tab_item("cook-tab-changes", "Files", Some(changes), ModTab::Changes, cx)).child(tab_item("cook-tab-details", "Details", None, ModTab::Details, cx)).child(tab_item("cook-tab-publish", "Publish", None, ModTab::Publish, cx));
        let name = self.meta.text("name", cx);
        div()
            .flex_none()
            .bg(theme.sidebar)
            .border_b_1()
            .border_color(theme.border)
            .child(
                page_width().pt_5().gap_4().child(
                    h_flex()
                        .flex_wrap()
                        .gap_4()
                        .child(crate::studio::ui::mod_icon(self.meta.icon.as_deref(), &id, &name, 56.0, &theme))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(220.0))
                                .gap_1()
                                .child(div().text_xl().font_weight(gpui::FontWeight::BOLD).overflow_hidden().text_ellipsis().whitespace_nowrap().child(title))
                                .child(div().text_sm().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(facts.join(" · "))),
                        )
                        .child(actions),
                )
                .child(tabs),
            )
            .into_any_element()
    }

    fn focus_summary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = ModTab::Publish;
        self.meta.summary.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    /// The right column, the same on every tab: the mod as the mod list shows it, what stops the build, the last build.
    fn render_rail(&mut self, title: &str, problems: &[String], cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let id = self.mod_id(cx);
        let name = self.meta.text("name", cx);
        let author = self.meta.text("author", cx);
        let summary = self.meta.text("summary", cx);
        let tags = modmeta::tags(&self.meta.text("tags", cx));
        let (kind, kind_icon) = self.kind();
        let signed = self.account.as_ref().is_some_and(|a| a.has_key);
        let preview = crate::studio::ui::panel(gpui_kit_assets::IconName::Eye, "In the mod list", "preview", &theme).child(
            v_flex()
                .gap_2()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .child(
                    h_flex()
                        .gap_3()
                        .child(crate::studio::ui::mod_icon(self.meta.icon.as_deref(), &id, &name, 48.0, &theme))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).overflow_hidden().text_ellipsis().whitespace_nowrap().child(title.to_string()))
                                .child(div().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(format!("{} · v{}", if author.is_empty() { "You" } else { &author }, self.version(cx)))),
                        ),
                )
                .child(if summary.is_empty() {
                    let placeholder = if self.tab == ModTab::Publish {
                        "No summary yet: write one under Listing."
                    } else {
                        "No summary yet: click to write one in Publish."
                    };
                    div()
                        .id("cook-summary-empty")
                        .relative()
                        .focusable()
                        .tab_index(0)
                        .cursor_pointer()
                        .rounded_sm()
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .text_xs()
                        .italic()
                        .text_color(theme.muted_foreground)
                        .whitespace_normal()
                        .hover(|s| s.text_color(theme.foreground).underline())
                        .focus_visible(|s| s.border_color(theme.ring).text_color(theme.foreground))
                        .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                            if matches!(e.keystroke.key.as_str(), "enter" | "space") && !e.keystroke.modifiers.modified() {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.focus_summary(window, cx);
                            }
                        }))
                        .on_click(cx.listener(|this, _, window, cx| this.focus_summary(window, cx)))
                        .child(placeholder)
                        .child(crate::driver::probe("cook-summary-empty"))
                        .into_any_element()
                } else {
                    div().text_xs().line_clamp(2).child(summary).into_any_element()
                })
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_1()
                        .child(crate::studio::ui::badge(Some(kind_icon), kind, theme.muted_foreground, &theme))
                        .child(if signed { crate::studio::ui::badge(Some(gpui_kit_assets::IconName::ShieldCheck), "Signed", theme.success, &theme) } else { crate::studio::ui::badge(None, "Unsigned", theme.muted_foreground, &theme) })
                        .children(tags.into_iter().take(8).map(|t| crate::studio::ui::badge(None, t, theme.muted_foreground, &theme))),
                ),
        );
        let mut issues: Vec<String> = problems.to_vec();
        if cli_path().is_none() {
            issues.push("The mod packer (tempest-cli) is missing from this Studio: reinstall the Studio, or set TEMPEST_CLI".into());
        }
        let n = issues.len();
        let mut check = crate::studio::ui::panel(if n == 0 { gpui_kit_assets::IconName::CircleCheck } else { gpui_kit_assets::IconName::TriangleAlert }, "Before building", if n == 0 { String::new() } else { format!("{n} to do") }, &theme);
        if n == 0 {
            check = check.child(h_flex().gap_2().text_sm().child(gpui_component::Icon::new(gpui_kit_assets::IconName::CircleCheck).small().text_color(theme.success)).child("Ready: Build & install puts it in the game."));
        }
        let (_, _, applying_movies) = self.unapplied_movies_status(cx);
        let applying_particles = self.applying_particles;
        let naming = self.project.is_none();
        for (k, p) in issues.into_iter().enumerate() {
            let unapplied_it = p.contains("unapplied stage edits");
            let unapplied_pt = p.to_ascii_lowercase().contains("unapplied particle curve edits");
            let name_it = naming && p.starts_with("Give the mod a name");
            let go = problem_tab(&p).filter(|t| *t != self.tab && !name_it && !unapplied_it && !unapplied_pt);
            check = check.child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(div().flex_none().pt(px(2.0)).child(gpui_component::Icon::new(gpui_kit_assets::IconName::CircleAlert).small().text_color(theme.warning)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_xs().whitespace_normal().child(p))
                            .when(unapplied_it, |d| {
                                d.child(
                                    h_flex().child(
                                        div()
                                            .relative()
                                            .child(
                                                Button::new(("cook-apply-movies", k))
                                                    .xsmall()
                                                    .label(if applying_movies { "Applying…" } else { "Apply now" })
                                                    .disabled(applying_movies)
                                                    .tooltip(if applying_movies { "Applying pending UI movie stage edits…" } else { "Apply pending UI movie stage edits so they enter your mod's changed files" })
                                                    .on_click(cx.listener(|this, _, _, cx| this.apply_unapplied_movies(cx))),
                                            )
                                            .child(crate::driver::probe("cook-apply-movies")),
                                    ),
                                )
                            })
                            .when(unapplied_pt, |d| {
                                d.child(
                                    h_flex().child(
                                        div()
                                            .relative()
                                            .child(
                                                Button::new(("cook-apply-particles", k))
                                                    .xsmall()
                                                    .label(if applying_particles { "Applying…" } else { "Apply now" })
                                                    .disabled(applying_particles)
                                                    .tooltip(if applying_particles { "Applying pending particle curve edits…" } else { "Write pending particle curve edits into the package so they enter your mod's changed files" })
                                                    .on_click(cx.listener(|this, _, _, cx| this.apply_unapplied_particles(cx))),
                                            )
                                            .child(crate::driver::probe("cook-apply-particles")),
                                    ),
                                )
                            })
                            .when(name_it, |d| {
                                d.child(h_flex().child(Button::new(("cook-fix", k)).xsmall().ghost().label("Name this mod…").icon(gpui_component::Icon::new(gpui_kit_assets::IconName::ArrowUpRight)).on_click(cx.listener(|this, _, window, cx| {
                                    this.run_studio(super::Cmd::NewProject, window, cx);
                                }))))
                            })
                            .when_some(go, |d, t| {
                                d.child(h_flex().child(Button::new(("cook-fix", k)).xsmall().ghost().label(if t == ModTab::Publish { "Open Publish" } else { "Open Details" }).icon(gpui_component::Icon::new(gpui_kit_assets::IconName::ArrowUpRight)).on_click(cx.listener(move |this, _, _, cx| {
                                    this.tab = t;
                                    cx.notify();
                                }))))
                            }),
                    ),
            );
        }
        let errors = self.log.iter().any(|(l, _)| *l == Line::Error);
        let show_log = self.show_log || errors;
        let last = (!self.log.is_empty()).then(|| {
            let mut log = v_flex().gap_0p5().p_2().rounded_md().bg(theme.background).max_h(px(240.0)).overflow_hidden();
            for (line, text) in &self.log {
                log = log.child(div().text_xs().font_family(crate::studio::ui::mono()).whitespace_normal().text_color(match line {
                    Line::Ok => theme.foreground,
                    Line::Info => theme.muted_foreground,
                    Line::Error => theme.danger,
                }).child(text.clone()));
            }
            let built = self.built.clone();
            let temp_fast = self.built_fast && !self.meta.fast;
            crate::studio::ui::panel(gpui_kit_assets::IconName::Hammer, "Last build", "", &theme)
                .child(div().text_xs().text_color(if errors { theme.danger } else { theme.muted_foreground }).whitespace_normal().child(match &built {
                    Some((path, signed)) => format!("{}{}{}", file_name(path), if *signed { ", signed" } else { ", unsigned" }, if temp_fast { " · fast test build" } else { "" }),
                    None if self.busy.is_some() => self.busy.clone().unwrap_or_default(),
                    None => "It did not finish: see the log.".into(),
                }))
                .child(
                    h_flex()
                        .gap_1()
                        .when_some(built.map(|b| b.0), |d, path| {
                            d.child(Button::new("cook-show-built").xsmall().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::FolderOpen)).label("Show the file").on_click(move |_, _, cx| cx.reveal_path(Path::new(&path))))
                        })
                        .when(!errors, |d| {
                            d.child(Button::new("cook-log").xsmall().ghost().label(if show_log { "Hide the log" } else { "Show the log" }).on_click(cx.listener(|this, _, _, cx| {
                                this.show_log = !this.show_log;
                                cx.notify();
                            })))
                        }),
                )
                .when(show_log, |d| d.child(log))
        });
        let publish = (self.tab == ModTab::Publish).then(|| {
            let pub_tip = if let Some(b) = &self.busy {
                b.clone()
            } else if self.built.is_none() {
                "Build your mod first, then upload it here".to_string()
            } else {
                "Upload the built mod to the mod repository".to_string()
            };
            h_flex()
                .gap_2()
                .child(Button::new("cook-publish").icon(gpui_component::Icon::new(gpui_kit_assets::IconName::Send)).label("Publish…").disabled(self.built.is_none() || self.busy.is_some()).tooltip(pub_tip).on_click(cx.listener(|this, _, window, cx| this.publish_dialog(window, cx))))
                .when(self.built.is_none(), |d| d.child(div().flex_1().min_w_0().child(crate::studio::ui::hint("Build the mod first (Build & install, or ⋯ ▸ Build only…), then publish it here.", &theme))))
        });
        v_flex().flex_none().w(px(RAIL)).gap_4().child(preview).children(publish).child(check).children(last).into_any_element()
    }
}

impl Cooker {
    /// The game patches it works with, progressively disclosed by mode: a calm confirmation for
    /// Every patch, two presets + dropdowns (with optional timeline) for A range, or era-grouped pills for Specific.
    fn render_compat(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit_assets::IconName;
        let theme = cx.theme().clone();
        let all = modmeta::patches();
        let latest = all.last().map(|p| p.version.clone()).unwrap_or_default();
        let mode = self.compat_mode;
        let show_timeline = self.show_timeline;
        let anchor = self.anchor.clone();
        let game_patch = self.game_patch.clone();
        let compat = self.meta.compat.clone();
        let mode_card = |id: &'static str, icon: IconName, label: &'static str, hint: &'static str, m: CompatMode, cx: &mut Context<Self>| {
            let on = mode == m;
            v_flex()
                .id(id)
                .relative()
                .flex_1()
                .flex_basis(px(140.0))
                .min_w(px(140.0))
                .p_2()
                .gap_0p5()
                .rounded_md()
                .border_1()
                .border_color(if on { theme.primary } else { theme.border })
                .bg(if on { theme.primary.opacity(0.1) } else { theme.background })
                .cursor_pointer()
                .focusable()
                .tab_index(0)
                .hover(|s| s.border_color(theme.primary.opacity(0.5)))
                .focus_visible(|s| s.border_1().border_color(theme.ring))
                .child(h_flex().gap_1p5().text_sm().when(on, |d| d.font_weight(gpui::FontWeight::SEMIBOLD)).child(gpui_component::Icon::new(icon).small().text_color(if on { theme.primary } else { theme.muted_foreground })).child(label))
                .child(div().text_xs().text_color(theme.muted_foreground).whitespace_normal().child(hint))
                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                    if matches!(e.keystroke.key.as_str(), "enter" | "space") && !e.keystroke.modifiers.modified() {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.set_compat_mode(m, cx);
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.set_compat_mode(m, cx)))
                .child(crate::driver::probe(id))
        };
        let modes = h_flex()
            .flex_wrap()
            .gap_2()
            .child(mode_card("compat-all", IconName::Infinity, "Every patch", "Any version of the game", CompatMode::All, cx))
            .child(mode_card("compat-range", IconName::MoveHorizontal, "A range", "From one patch to another", CompatMode::Range, cx))
            .child(mode_card("compat-some", IconName::ListChecks, "Specific", "Only the patches you pick", CompatMode::Specific, cx));

        if mode == CompatMode::All {
            return v_flex()
                .gap_3()
                .child(modes)
                .child(crate::studio::ui::hint("Works with every released and future Paladins patch.", &theme))
                .into_any_element();
        }

        let mut range_controls: Option<AnyElement> = None;
        let mut range_disclosure: Option<AnyElement> = None;
        if mode == CompatMode::Range {
            let target_patch = game_patch.clone().unwrap_or_else(|| latest.clone());
            let has_game_patch = game_patch.is_some();
            let preset_newer_on = compat.patches.is_empty() && compat.from.as_deref() == Some(target_patch.as_str()) && compat.to.is_none();
            let preset_only_on = compat.patches.is_empty() && compat.from.as_deref() == Some(target_patch.as_str()) && compat.to.as_deref() == Some(target_patch.as_str());
            let (tp1, tp2) = (target_patch.clone(), target_patch.clone());
            let presets = h_flex()
                .flex_wrap()
                .gap_1p5()
                .child(
                    Button::new("compat-preset-newer")
                        .xsmall()
                        .ghost()
                        .selected(preset_newer_on)
                        .label(if has_game_patch { "This game's patch & newer" } else { "Latest patch & newer" })
                        .tooltip(format!("Paladins {target_patch} and any newer patch"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.meta.compat = modmeta::Compat { patches: Vec::new(), from: Some(tp1.clone()), to: None };
                            this.compat_mode = CompatMode::Range;
                            this.anchor = None;
                            this.save_meta(cx);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("compat-preset-only")
                        .xsmall()
                        .ghost()
                        .selected(preset_only_on)
                        .label(if has_game_patch { "Only this game's patch" } else { "Only latest patch" })
                        .tooltip(format!("Only Paladins {target_patch}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.meta.compat = modmeta::Compat { patches: Vec::new(), from: Some(tp2.clone()), to: Some(tp2.clone()) };
                            this.compat_mode = CompatMode::Range;
                            this.anchor = None;
                            this.save_meta(cx);
                            cx.notify();
                        })),
                );

            let from_val = compat.from.clone().unwrap_or_else(|| target_patch.clone());
            let to_val = compat.to.clone();
            let describe_patch = |v: &str| match all.iter().find(|p| p.version == v) {
                Some(p) if !p.name.is_empty() => format!("{} · {}", p.version, p.name),
                Some(p) => p.version.clone(),
                None => v.to_string(),
            };
            let me_from = cx.entity().downgrade();
            let cur_from = from_val.clone();
            let from_dropdown = Button::new("compat-from")
                .small()
                .w_full()
                .dropdown_caret(true)
                .label(describe_patch(&from_val))
                .dropdown_menu(move |menu, _, _| {
                    let mut m = menu.scrollable(true).max_h(px(320.0));
                    for p in modmeta::patches().iter().rev() {
                        let v = p.version.clone();
                        let me = me_from.clone();
                        let label = if p.name.is_empty() { p.version.clone() } else { format!("{} · {}", p.version, p.name) };
                        m = m.item(PopupMenuItem::new(label).checked(cur_from == p.version).on_click(move |_, _, cx| {
                            let v = v.clone();
                            let _ = me.update(cx, |this, cx| {
                                this.meta.compat.patches.clear();
                                if this.meta.compat.to.as_deref().is_some_and(|t| modmeta::order(t) < modmeta::order(&v)) {
                                    this.meta.compat.to = Some(v.clone());
                                }
                                this.meta.compat.from = Some(v);
                                this.anchor = None;
                                this.save_meta(cx);
                                cx.notify();
                            });
                        }));
                    }
                    m
                });

            let me_to = cx.entity().downgrade();
            let cur_to = to_val.clone();
            let to_label = match &to_val {
                Some(t) => describe_patch(t),
                None => "And newer (future patches too)".to_string(),
            };
            let to_dropdown = Button::new("compat-to")
                .small()
                .w_full()
                .dropdown_caret(true)
                .label(to_label)
                .dropdown_menu(move |menu, _, _| {
                    let me_future = me_to.clone();
                    let mut m = menu
                        .scrollable(true)
                        .max_h(px(320.0))
                        .item(PopupMenuItem::new("And newer (future patches too)").checked(cur_to.is_none()).on_click(move |_, _, cx| {
                            let _ = me_future.update(cx, |this, cx| {
                                this.meta.compat.to = None;
                                this.anchor = None;
                                this.save_meta(cx);
                                cx.notify();
                            });
                        }))
                        .separator();
                    for p in modmeta::patches().iter().rev() {
                        let v = p.version.clone();
                        let me = me_to.clone();
                        let label = if p.name.is_empty() { p.version.clone() } else { format!("{} · {}", p.version, p.name) };
                        m = m.item(PopupMenuItem::new(label).checked(cur_to.as_deref() == Some(p.version.as_str())).on_click(move |_, _, cx| {
                            let v = v.clone();
                            let _ = me.update(cx, |this, cx| {
                                this.meta.compat.patches.clear();
                                if this.meta.compat.from.as_deref().is_none_or(|f| modmeta::order(f) > modmeta::order(&v)) {
                                    this.meta.compat.from = Some(v.clone());
                                }
                                this.meta.compat.to = Some(v);
                                this.anchor = None;
                                this.save_meta(cx);
                                cx.notify();
                            });
                        }));
                    }
                    m
                });

            range_controls = Some(
                v_flex()
                    .gap_2()
                    .child(presets)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_start()
                            .gap_3()
                            .child(div().flex_1().min_w(px(180.0)).child(crate::studio::ui::field("From patch", "", from_dropdown, &theme)))
                            .child(div().flex_1().min_w(px(180.0)).child(crate::studio::ui::field("To patch", "", to_dropdown, &theme))),
                    )
                    .into_any_element(),
            );
            range_disclosure = Some(
                h_flex()
                    .child(
                        Button::new("compat-timeline-toggle")
                            .xsmall()
                            .ghost()
                            .icon(gpui_component::Icon::new(if show_timeline { IconName::ChevronDown } else { IconName::ChevronRight }))
                            .label(if show_timeline { "Hide patch timeline" } else { "Pick on visual patch timeline…" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_timeline = !this.show_timeline;
                                if !this.show_timeline {
                                    this.anchor = None;
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }

        let timeline: Option<AnyElement> = ((mode == CompatMode::Range && show_timeline) || mode == CompatMode::Specific).then(|| {
            let mut eras: Vec<(String, Vec<(usize, &modmeta::Patch)>)> = Vec::new();
            for (i, p) in all.iter().enumerate() {
                let major = p.version.split('.').next().unwrap_or_default().to_string();
                match eras.last_mut() {
                    Some((m, list)) if *m == major => list.push((i, p)),
                    _ => eras.push((major, vec![(i, p)])),
                }
            }
            let mut grid = v_flex().gap_1p5().w_full();
            for (major, list) in eras {
                let mut pills = h_flex().flex_1().min_w_0().flex_wrap().gap_1();
                for (i, p) in &list {
                    let on = compat.covers(&p.version);
                    let anchored = anchor.as_deref() == Some(p.version.as_str());
                    let endpoint = mode == CompatMode::Range && (compat.from.as_deref() == Some(p.version.as_str()) || compat.to.as_deref() == Some(p.version.as_str()));
                    let tip = if p.name.is_empty() { p.version.clone() } else { format!("{} · {}", p.version, p.name) };
                    let (v, v2) = (p.version.clone(), p.version.clone());
                    let (bg, border, fg, bold) = if anchored {
                        (theme.primary, theme.ring, theme.primary_foreground, true)
                    } else if on && (mode == CompatMode::Specific || endpoint) {
                        (theme.primary.opacity(0.16), theme.primary, theme.foreground, true)
                    } else if on {
                        (theme.primary.opacity(0.10), theme.primary.opacity(0.6), theme.foreground, false)
                    } else {
                        (theme.secondary, theme.border, theme.muted_foreground, false)
                    };
                    let hover_fg = if anchored { theme.primary_foreground } else { theme.foreground };
                    pills = pills.child(
                        div()
                            .id(("tick", *i))
                            .min_h(px(24.0))
                            .py_0p5()
                            .min_w(px(34.0))
                            .px_1p5()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(border)
                            .bg(bg)
                            .text_xs()
                            .font_family(crate::studio::ui::mono())
                            .text_color(fg)
                            .when(bold, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                            .whitespace_nowrap()
                            .cursor_pointer()
                            .focusable()
                            .tab_index(0)
                            .hover(|s| s.border_1().border_color(theme.primary).text_color(hover_fg))
                            .focus_visible(|s| s.border_1().border_color(theme.ring))
                            .child(p.version.clone())
                            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                if matches!(e.keystroke.key.as_str(), "enter" | "space") && !e.keystroke.modifiers.modified() {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.click_patch(v2.clone(), cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| this.click_patch(v.clone(), cx)))
                            .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)),
                    );
                }
                grid = grid.child(
                    h_flex()
                        .items_start()
                        .gap_2()
                        .w_full()
                        .child(div().flex_none().min_w(px(38.0)).min_h(px(24.0)).py_0p5().flex().items_center().text_xs().font_family(crate::studio::ui::mono()).text_color(theme.muted_foreground).whitespace_nowrap().child(format!("{major}.x")))
                        .child(pills),
                );
            }
            grid.into_any_element()
        });

        let how: Option<AnyElement> = match mode {
            CompatMode::All => None,
            CompatMode::Range if show_timeline => Some(
                div()
                    .text_xs()
                    .text_color(if anchor.is_some() { theme.foreground } else { theme.muted_foreground })
                    .whitespace_normal()
                    .child(match &anchor {
                        Some(a) => format!("From {a}: now click the last patch of the range."),
                        None => "Click the first patch of the range, then the last.".to_string(),
                    })
                    .into_any_element(),
            ),
            CompatMode::Range => None,
            CompatMode::Specific => {
                let mut chips = h_flex().gap_2().flex_wrap().text_xs();
                if compat.patches.is_empty() {
                    chips = chips.child(div().text_color(theme.muted_foreground).child("Click patches above to add them."));
                }
                for (k, v) in compat.patches.iter().enumerate() {
                    let name = all.iter().find(|p| &p.version == v).map(|p| p.name.clone()).unwrap_or_default();
                    let (v2, v3) = (v.clone(), v.clone());
                    chips = chips.child(
                        h_flex()
                            .id(("compat-chip", k))
                            .min_h(px(24.0))
                            .py_0p5()
                            .px_2()
                            .gap_1()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.primary)
                            .bg(theme.primary.opacity(0.16))
                            .text_color(theme.foreground)
                            .cursor_pointer()
                            .focusable()
                            .tab_index(0)
                            .hover(|s| s.bg(theme.primary.opacity(0.24)))
                            .focus_visible(|s| s.border_1().border_color(theme.ring))
                            .child(div().font_family(crate::studio::ui::mono()).font_weight(gpui::FontWeight::SEMIBOLD).child(v.clone()))
                            .when(!name.is_empty(), |d| d.child(name))
                            .child(gpui_component::Icon::new(IconName::X).xsmall().text_color(theme.muted_foreground))
                            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                if matches!(e.keystroke.key.as_str(), "enter" | "space" | "backspace" | "delete") && !e.keystroke.modifiers.modified() {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.click_patch(v3.clone(), cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| this.click_patch(v2.clone(), cx))),
                    );
                }
                Some(chips.into_any_element())
            }
        };

        let covered = compat.covered();
        let summary = if covered.len() == all.len() && compat.to.is_none() {
            "Works on every patch, including future ones.".to_string()
        } else if covered.len() == 1 {
            format!("Works on 1 patch (of {}): {}", all.len(), modmeta::group_patches(&covered).join(", "))
        } else if !covered.is_empty() {
            format!("Works on {} of {} patches: {}", covered.len(), all.len(), modmeta::group_patches(&covered).join(", "))
        } else if mode == CompatMode::Specific {
            "No patches picked yet.".to_string()
        } else if compat.is_set() {
            format!("This range ({}) covers no known patch.", compat.describe())
        } else {
            "Pick the patches it works with.".to_string()
        };
        v_flex()
            .gap_3()
            .child(modes)
            .children(range_controls)
            .children(range_disclosure)
            .children(timeline)
            .children(how)
            .child(div().text_xs().text_color(if covered.is_empty() { theme.warning } else { theme.muted_foreground }).whitespace_normal().child(summary))
            .into_any_element()
    }

    fn set_compat_mode(&mut self, mode: CompatMode, cx: &mut Context<Self>) {
        let all = modmeta::patches();
        let first = all.first().map(|p| p.version.clone());
        let c = &mut self.meta.compat;
        match mode {
            CompatMode::All => *c = modmeta::Compat { patches: Vec::new(), from: first, to: None },
            // A range keeps its ends; from Every patch it starts at the open game's patch (else 0.56).
            CompatMode::Range => {
                c.patches.clear();
                if self.compat_mode == CompatMode::All || c.from.is_none() {
                    c.from = self.game_patch.clone().or_else(|| all.iter().find(|p| p.version == "0.56").or(all.first()).map(|p| p.version.clone()));
                    c.to = None;
                }
            }
            CompatMode::Specific => {
                c.from = None;
                c.to = None;
            }
        }
        self.compat_mode = mode;
        self.anchor = None;
        self.save_meta(cx);
        cx.notify();
    }

    /// A click on a patch: a range's first then last end, or a patch picked or put back.
    fn click_patch(&mut self, version: String, cx: &mut Context<Self>) {
        match self.compat_mode {
            CompatMode::All => return,
            CompatMode::Specific => {
                let list = &mut self.meta.compat.patches;
                if let Some(k) = list.iter().position(|p| *p == version) {
                    list.remove(k);
                } else {
                    list.push(version);
                    list.sort_by_key(|p| modmeta::order(p));
                }
            }
            CompatMode::Range => match self.anchor.take() {
                None => self.anchor = Some(version),
                Some(a) => {
                    let (lo, hi) = if modmeta::order(&a) <= modmeta::order(&version) { (a, version) } else { (version, a) };
                    self.meta.compat = modmeta::Compat { patches: Vec::new(), from: Some(lo), to: Some(hi) };
                }
            },
        }
        self.save_meta(cx);
        cx.notify();
    }
}

/// The right column's width, as on the website's mod page.
const RAIL: f32 = 300.0;

/// The Mod view's page width: the header and the page share its left edge.
fn page_width() -> gpui::Div {
    v_flex().w_full().max_w(px(1160.0)).mx_auto().px_6()
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
        let input = |i: &Entity<InputState>| Input::new(i).small();
        let problems = self.problems(cx);
        let name = self.meta.text("name", cx);
        let title = if name.is_empty() { self.project.as_ref().map(|p| p.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Unnamed mod".into()) } else { name };
        let header = self.render_header(title.clone(), &problems, cx);
        let rail = self.render_rail(&title, &problems, cx);
        let id = self.mod_id(cx);
        let main = v_flex().flex_grow(1.0).flex_basis(px(480.0)).min_w(px(360.0)).gap_4();
        let main = match self.tab {
            ModTab::Changes => {
                let files = self.render_files(cx);
                let extras = self.render_extras(cx);
                let n = self.targets.len();
                main.child(
                    crate::studio::ui::panel(gpui_kit_assets::IconName::Pencil, "Game files you changed", h_flex().gap_2().when(n > 0, |d| d.child(format!("{n} file{}", if n == 1 { "" } else { "s" }))).child(Button::new("cook-refresh").xsmall().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::RefreshCw)).tooltip("Look for changes again").on_click(cx.listener(|this, _, _, cx| this.refresh(cx)))), &theme)
                        .when(n > 0, |d| d.child(crate::studio::ui::hint("They go into the mod as they are in the Studio; the game's own files stay untouched.", &theme)))
                        .child(files),
                )
                .child(
                    crate::studio::ui::panel(gpui_kit_assets::IconName::FilePlus, "Other files", h_flex().gap_1().child("optional").child(Button::new("add-files").xsmall().ghost().label("Add files…").tooltip("Game packages (.upk), data tables (.dat), sound banks (.pck), Lua scripts (scripts/), or DLLs (dlls/64/)").on_click(cx.listener(|this, _, _, cx| this.add_files(cx)))).child(Button::new("add-folder").xsmall().ghost().label("Add folder…").tooltip("A whole folder, shipped under its name (for example scripts/)").on_click(cx.listener(|this, _, _, cx| this.add_folder(cx)))), &theme)
                        .child(extras),
                )
            }
            ModTab::Details => {
                let v = self.meta.text("version", cx);
                let fix = (!v.is_empty() && !modmeta::is_semver(&v)).then(|| modmeta::to_semver(&v)).flatten();
                // A typed id that breaks the rule: offer the same id made valid, when there is one.
                let typed_id = self.meta.text("id", cx);
                let id_fix = Some(super::project::id_of(&typed_id)).filter(|f| !typed_id.is_empty() && *f != typed_id && modmeta::id_ok(f));
                let lua = self.ships_lua();
                let fast = self.meta.fast;
                let pick = |id: &'static str, label: &'static str, tip: &'static str, value: bool, cx: &mut Context<Self>| {
                    Button::new(id).small().label(label).tooltip(tip).ghost().selected(fast == value).on_click(cx.listener(move |this, _, _, cx| {
                        this.meta.fast = value;
                        this.save_meta(cx);
                        cx.notify();
                    }))
                };
                main.child(
                    crate::studio::ui::panel(gpui_kit_assets::IconName::Pencil, "The mod", "", &theme)
                        .child(h_flex().items_start().gap_3().child(div().flex_grow(1.0).flex_basis(px(0.0)).flex_shrink(1.0).min_w_0().child(crate::studio::ui::field("Name", "", input(&self.meta.name), &theme))).child(div().w(px(140.0)).flex_none().child(crate::studio::ui::field("Version", "", input(&self.meta.version), &theme))))
                        .when_some(fix, |d, v| {
                            d.child(h_flex().gap_2().child(div().flex_1().min_w_0().text_xs().text_color(theme.warning).child(format!("The launcher reads versions like 1.0.0: did you mean {v}?"))).child(Button::new("cook-fix-version").xsmall().label(format!("Use {v}")).on_click(cx.listener(move |this, _, window, cx| {
                                let v = v.clone();
                                this.meta.version.update(cx, |s, cx| s.set_value(v, window, cx));
                                this.save_meta(cx);
                            }))))
                        })
                        .child(h_flex().items_start().gap_3().child(crate::studio::ui::field("Author", "", input(&self.meta.author), &theme)).child(crate::studio::ui::field("Author link", "optional", input(&self.meta.link), &theme)))
                        .when_some(modmeta::author_link_problem(&self.meta.text("link", cx)), |d, why| d.child(div().text_xs().text_color(theme.warning).child(why))),
                )
                .child(crate::studio::ui::panel(gpui_kit_assets::IconName::Image, "Icon", "optional", &theme).child(self.render_icon(cx)))
                .child(crate::studio::ui::panel(gpui_kit_assets::IconName::BookOpen, "Readme", "optional · Markdown", &theme).child(Textarea::new(&self.meta.readme)))
                .child(
                    crate::studio::ui::panel(gpui_kit_assets::IconName::Settings2, "Advanced", "", &theme)
                        .child(
                            v_flex()
                                .gap_1()
                                .child(crate::studio::ui::field("Id", "", input(&self.meta.id), &theme))
                                .child(match (self.id_problem(cx), id_fix) {
                                    (Some(why), fix) => h_flex()
                                        .gap_2()
                                        .child(div().flex_1().min_w_0().text_xs().text_color(theme.danger).child(why))
                                        .when_some(fix, |d, fix| {
                                            d.child(Button::new("cook-fix-id").xsmall().label(format!("Use {fix}")).on_click(cx.listener(move |this, _, window, cx| {
                                                let fix = fix.clone();
                                                this.meta.id.update(cx, |s, cx| s.set_value(fix, window, cx));
                                                this.save_meta(cx);
                                                cx.notify();
                                            })))
                                        })
                                        .into_any_element(),
                                    (None, _) => crate::studio::ui::hint(format!("How launchers and other mods name this mod: {id}. Made from the name unless you type one."), &theme).into_any_element(),
                                }),
                        )
                        .child(v_flex().gap_1().child(crate::studio::ui::field("Mods it needs", "optional", Textarea::new(&self.meta.deps), &theme)).child(crate::studio::ui::hint(if lua { "tempest-scripting>=1.0.0 is added for you: the mod ships Lua." } else { "One per line: an id, then a version requirement like >=1.2.0, ^2 or ~1.2." }, &theme)))
                        .child(
                            v_flex()
                                .gap_1()
                                .child(div().text_xs().text_color(theme.muted_foreground).child("Compression (Build only)"))
                                .child(h_flex().gap_1().child(pick("cook-small", "Smallest file", "LZMA: the smallest mod, slower to build (a big mod takes a minute or so)", false, cx)).child(pick("cook-fast", "Faster build", "zstd: builds several times faster, the mod is about 30 % bigger", true, cx)))
                                .child(crate::studio::ui::hint("Build & install always builds fast. Build the smallest file for sharing and publishing.", &theme)),
                        ),
                )
            }
            ModTab::Publish => {
                let summary_len = self.meta.text("summary", cx).chars().count();
                let repo = self.account.as_ref().map(|a| a.repo_url.clone()).unwrap_or_else(|| "https://mods.lowrezstudio.com".into());
                let account = self.render_account(cx);
                let compat = self.render_compat(cx);
                main.child(
                    crate::studio::ui::panel(gpui_kit_assets::IconName::Store, "Listing", "", &theme)
                        .child(crate::studio::ui::hint("How the mod shows in Tempest Launcher and on the website once it is published: see the preview on the right.", &theme))
                        .child(crate::studio::ui::field("Summary", div().when(summary_len > modmeta::MAX_SUMMARY, |d| d.text_color(theme.danger)).child(format!("{summary_len} / {}", modmeta::MAX_SUMMARY)), input(&self.meta.summary), &theme))
                        .child(crate::studio::ui::field("Tags", "comma separated, 8 at most", input(&self.meta.tags), &theme))
                        .when_some(modmeta::tags_problem(&self.meta.text("tags", cx)), |d, why| d.child(div().text_xs().text_color(theme.danger).child(why)))
                        .child(crate::studio::ui::field("Source link", "optional", input(&self.meta.repository), &theme))
                        .when_some(modmeta::repository_problem(&self.meta.text("repository", cx)), |d, why| d.child(div().text_xs().text_color(theme.danger).child(why))),
                )
                .child(
                    crate::studio::ui::panel(gpui_kit_assets::IconName::Gamepad2, "Game patches it works with", "", &theme).child(compat),
                )
                .child(crate::studio::ui::panel(gpui_kit_assets::IconName::FileText, "What changed in this version", "optional", &theme).child(Textarea::new(&self.meta.changelog)))
                .child(
                    crate::studio::ui::panel(gpui_kit_assets::IconName::Image, "Banner, screenshots and visibility", "", &theme)
                        .child(crate::studio::ui::hint("These are set on the mod's page on the website, after the first upload, along with more authors and who can see it.", &theme))
                        .child(h_flex().child(Button::new("cook-website").xsmall().ghost().icon(gpui_component::Icon::new(gpui_kit_assets::IconName::ExternalLink)).label(repo.trim_start_matches("https://").to_string()).on_click(move |_, _, cx| cx.open_url(&format!("{repo}/dashboard"))))),
                )
                .child(crate::studio::ui::panel(gpui_kit_assets::IconName::ShieldCheck, "Signing and account", "", &theme).child(account))
            }
        };
        let page = page_width().py_5().child(h_flex().items_start().flex_wrap().gap_6().child(main).child(rail));
        v_flex().size_full().bg(theme.background).child(header).child(div().id("cook-page").flex_1().min_h_0().overflow_y_scroll().child(page))
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
                                        Err(e) => this.toast(Tone::Error, super::ui::explain_error(&e), cx),
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
        assert_eq!(file_in_words("ChaosGame/CookedPCConsole/Lang_INT.dat"), "English texts (Lang_INT.dat)");
        assert_eq!(file_in_words("ChaosGame/CookedPCConsole/Lang_FRA.dat"), "French texts (Lang_FRA.dat)");
        assert_eq!(file_in_words("ChaosGame/CookedPCConsole/Lang_DEU.dat"), "German texts (Lang_DEU.dat)");
        assert_eq!(file_in_words("ChaosGame/CookedPCConsole/CASSIE_ASSETS_SF.upk"), "CASSIE_ASSETS_SF");
    }

    #[test]
    fn detects_patch_from_game_path() {
        assert_eq!(detect_patch_in_path("/games/8.1/ChaosGame/CookedPCConsole").as_deref(), Some("8.1"));
        assert_eq!(detect_patch_in_path("/games/Paladins 0.57/ChaosGame/CookedPCConsole").as_deref(), Some("0.57"));
        assert_eq!(detect_patch_in_path("C:\\Games\\paladins-7.2\\ChaosGame\\CookedPCConsole").as_deref(), Some("7.2"));
        assert_eq!(detect_patch_in_path("/games/Paladins/ChaosGame/CookedPCConsole"), None);
    }

    #[test]
    fn explains_errors_idempotently() {
        let once = explain("Could not open x: No such file or directory (os error 2)");
        assert!(once.contains("missing"));
        assert_eq!(explain(&once), once);
    }

    #[test]
    fn formats_unapplied_movie_warning_singular_and_plural() {
        assert_eq!(Cooker::unapplied_movie_problem(1), "1 UI movie has unapplied stage edits");
        assert_eq!(Cooker::unapplied_movie_problem(3), "3 UI movies have unapplied stage edits");
        assert_eq!(problem_tab(&Cooker::unapplied_movie_problem(1)), None);
    }
}
