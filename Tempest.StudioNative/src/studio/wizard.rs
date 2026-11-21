//! **Create** (⌘N): one guided way to make anything new, step by step.
//! The first step asks what to make: a texture, mesh, sound or movie from a
//! file, a material instance, a copy of an object, a package or a group, or
//! game content (an ability, card, item, champion or skin). Each kind then
//! asks only what it needs (the file, the object it is modelled on, where
//! it goes and its name), shows what will happen, and makes it as unsaved,
//! undoable edits. Objects are always made like an existing one of their
//! kind (`object.add` with a template), so their settings fit the game.
//! Scripts: a Lua script for Tempest Scripting (a starter file in the
//! project's `mod-files/scripts/`, or a folder you pick) or an UnrealScript
//! class compiled into a script package (`script.compile {create}`).

use gpui::{div, prelude::*, px, AnyElement, App, Context, Entity, ExternalPaths, FocusHandle, PathPromptOptions, SharedString, Subscription, WeakEntity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::DropdownMenu;
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable, Selectable};
use gpui_kit_assets::IconName;
use serde_json::{json, Value as J};

use super::{file_name, Place, Studio, Tone};
use crate::backend::Backend;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Texture,
    StaticMesh,
    SkeletalMesh,
    Sound,
    Movie,
    MaterialInstance,
    Copy,
    Package,
    Group,
    Ability,
    Card,
    Item,
    Champion,
    Skin,
    LuaScript,
    UnrealClass,
}

/// What a new Lua script starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Starter {
    Event,
    Command,
    Key,
    Empty,
}

impl Starter {
    const ALL: [Starter; 4] = [Starter::Event, Starter::Command, Starter::Key, Starter::Empty];

    fn title(self) -> &'static str {
        match self {
            Starter::Event => "Game event",
            Starter::Command => "Console command",
            Starter::Key => "Key binding",
            Starter::Empty => "Empty",
        }
    }

    fn body(self, name: &str) -> String {
        let head = format!("-- {name}.lua: runs in the game with Tempest Scripting. Dev copies in <game>/mods/<id>/scripts/\n-- reload when saved; the API is in Tempest.Scripting/README.md.\n\n");
        head + match self {
            Starter::Event => "-- After a player joins a match: `this` is the game, `params` the call's parameters.\ntempest.on(\"PostLogin\", function(this, params)\n\tlocal pc = params.NewPlayer\n\tif pc and pc.PlayerReplicationInfo then\n\t\ttempest.log(\"joined: \" .. tostring(pc.PlayerReplicationInfo.PlayerName))\n\tend\nend)\n",
            Starter::Command => "-- Type `hello` (or `hello Kyiro`) in the game's console.\ntempest.command(\"hello\", function(pc, rest, argv)\n\ttempest.toast(\"Hello\" .. (rest ~= \"\" and (\", \" .. rest) or \"\") .. \"!\")\nend, \"hello [name]: says hello\")\n",
            Starter::Key => "-- Ctrl+F6 in the game; returning true keeps the key from the game.\ntempest.on_key(\"Ctrl+F6\", function()\n\ttempest.toast(\"Ctrl+F6 pressed\")\n\treturn true\nend)\n",
            Starter::Empty => "local cfg = tempest.config({})\n",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Choose,
    File,
    Champion,
    Model,
    Textures,
    Place,
    Details,
    Review,
}

impl Step {
    fn title(self) -> &'static str {
        match self {
            Step::Choose => "What to make",
            Step::File => "The file",
            Step::Champion => "The champion",
            Step::Model => "Based on",
            Step::Textures => "Textures",
            Step::Place => "Name and place",
            Step::Details => "Name and text",
            Step::Review => "Check and create",
        }
    }
}

impl Kind {
    const PACKAGE: &'static [Kind] = &[Kind::Texture, Kind::StaticMesh, Kind::SkeletalMesh, Kind::MaterialInstance, Kind::Sound, Kind::Movie, Kind::Copy, Kind::Package, Kind::Group];
    const CONTENT: &'static [Kind] = &[Kind::Skin, Kind::Ability, Kind::Card, Kind::Item, Kind::Champion];
    const SCRIPTS: &'static [Kind] = &[Kind::LuaScript, Kind::UnrealClass];
    const CHOOSE_DATA: &'static [Kind] = &[Kind::Skin, Kind::Ability, Kind::Card, Kind::Item];
    const CHOOSE_CHAMP_MEDIA: &'static [Kind] = &[Kind::Champion, Kind::Sound, Kind::Movie, Kind::Copy];
    const CHOOSE_MODELS: &'static [Kind] = &[Kind::Texture, Kind::MaterialInstance, Kind::StaticMesh, Kind::SkeletalMesh];
    const CHOOSE_CODE_PKGS: &'static [Kind] = &[Kind::Package, Kind::Group, Kind::UnrealClass, Kind::LuaScript];
    const CONTENT_CHAMPS: &'static [Kind] = &[Kind::Skin, Kind::Champion];
    const CONTENT_GAMEPLAY: &'static [Kind] = &[Kind::Ability, Kind::Card, Kind::Item];
    const PACKAGE_MEDIA_COPY: &'static [Kind] = &[Kind::Sound, Kind::Movie, Kind::Copy];
    const PACKAGE_CONTAINERS: &'static [Kind] = &[Kind::Package, Kind::Group];

    fn title(self) -> &'static str {
        match self {
            Kind::Texture => "Texture",
            Kind::StaticMesh => "Static mesh",
            Kind::SkeletalMesh => "Skeletal mesh",
            Kind::Sound => "Sound",
            Kind::Movie => "UI movie",
            Kind::MaterialInstance => "Material instance",
            Kind::Copy => "Copy of an object",
            Kind::Package => "Package",
            Kind::Group => "Group",
            Kind::Ability => "Ability",
            Kind::Card => "Card or talent",
            Kind::Item => "Item",
            Kind::Champion => "Champion",
            Kind::Skin => "Skin",
            Kind::LuaScript => "Lua script",
            Kind::UnrealClass => "UnrealScript class",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Kind::Texture => "From a PNG or DDS image",
            Kind::StaticMesh => "From a glTF, FBX or OBJ model",
            Kind::SkeletalMesh => "From a rigged glTF or FBX model",
            Kind::Sound => "From an Ogg or WAV file",
            Kind::Movie => "From a GFX or SWF movie",
            Kind::MaterialInstance => "A material look with your textures",
            Kind::Copy => "Any object, with what it uses",
            Kind::Package => "An empty package for your objects",
            Kind::Group => "A folder inside a package",
            Kind::Ability => "An ability copied from a champion's",
            Kind::Card => "A loadout card or talent",
            Kind::Item => "A spray, emote, frame…",
            Kind::Champion => "A whole champion, from another",
            Kind::Skin => "A skin with every part",
            Kind::LuaScript => "Runs in the game with Tempest Scripting",
            Kind::UnrealClass => "A new class in a script package",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Kind::Texture => IconName::Image,
            Kind::StaticMesh | Kind::SkeletalMesh => IconName::Box,
            Kind::Sound => IconName::Music,
            Kind::Movie => IconName::Film,
            Kind::MaterialInstance => IconName::Palette,
            Kind::Copy => IconName::Copy,
            Kind::Package => IconName::PackagePlus,
            Kind::Group => IconName::FolderPlus,
            Kind::Ability | Kind::Card => IconName::Sparkles,
            Kind::Item => IconName::Store,
            Kind::Champion => IconName::Wand,
            Kind::Skin => IconName::Shirt,
            Kind::LuaScript => IconName::SquareTerminal,
            Kind::UnrealClass => IconName::FileCode,
        }
    }

    fn steps(self) -> &'static [Step] {
        use Step::*;
        match self {
            Kind::Texture | Kind::StaticMesh | Kind::SkeletalMesh | Kind::Sound | Kind::Movie => &[Choose, File, Model, Place, Review],
            Kind::MaterialInstance => &[Choose, Model, Textures, Place, Review],
            Kind::Copy => &[Choose, Model, Place, Review],
            Kind::Package => &[Choose, Details, Review],
            Kind::Group => &[Choose, Place, Review],
            Kind::Ability | Kind::Card | Kind::Skin => &[Choose, Champion, Model, Details, Review],
            Kind::Item | Kind::Champion => &[Choose, Model, Details, Review],
            Kind::LuaScript | Kind::UnrealClass => &[Choose, Details, Review],
        }
    }

    /// The driver's names: `texture`, `static-mesh`, `material-instance`…
    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::PACKAGE.iter().chain(Kind::CONTENT).chain(Kind::SCRIPTS).copied().find(|k| format!("{k:?}").eq_ignore_ascii_case(&name.replace('-', "")))
    }

    fn content(self) -> bool {
        Kind::CONTENT.contains(&self)
    }

    /// The class the model must be (the search's `isA`).
    fn class(self) -> Option<&'static str> {
        Some(match self {
            Kind::Texture => "Texture2D",
            Kind::StaticMesh => "StaticMesh",
            Kind::SkeletalMesh => "SkeletalMesh",
            Kind::Sound => "SoundNodeWave",
            Kind::Movie => "SwfMovie",
            Kind::MaterialInstance => "MaterialInterface",
            _ => return None,
        })
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Kind::Texture => &["png", "dds", "tga"],
            Kind::StaticMesh => &["glb", "gltf", "fbx", "obj"],
            Kind::SkeletalMesh => &["glb", "gltf", "fbx"],
            Kind::Sound => &["ogg", "wav"],
            Kind::Movie => &["gfx", "swf"],
            _ => &[],
        }
    }

    /// The name's usual prefix in the game's packages.
    fn prefix(self) -> &'static str {
        match self {
            Kind::Texture => "t_",
            Kind::StaticMesh => "sm_",
            Kind::SkeletalMesh => "skl_",
            Kind::MaterialInstance => "mic_",
            _ => "",
        }
    }

    /// What the model step explains.
    fn model_hint(self) -> &'static str {
        match self {
            Kind::Texture => "The new texture is made like this one: compression, LOD group, sRGB and mip settings. Pick one used the same way (an icon for an icon, a normal map for a normal map).",
            Kind::StaticMesh | Kind::SkeletalMesh => "The new mesh is made like this one: its materials, LOD settings and, for skeletal meshes, the skeleton your model must be rigged to.",
            Kind::Sound => "The new sound is made like this one: its group, volume and attenuation.",
            Kind::Movie => "The new movie is made like this one.",
            Kind::MaterialInstance => "The material it is based on: the instance draws like it until you change its textures. Pick the material a similar object uses.",
            Kind::Copy => "The object to copy. What it uses in its package (materials, textures) is copied along.",
            Kind::Ability | Kind::Card => "The ability, card or talent to copy: its modes, effects and classes come along. Pick the one closest to what you want.",
            Kind::Item => "The item to copy (its skin row too).",
            Kind::Champion => "The champion to copy: the bot, an ability per slot, the class and weapon items and a default skin.",
            Kind::Skin => "The skin collection to copy: the container, every part (body, head, weapon, voice…) and its vendor entry.",
            _ => "",
        }
    }
}

/// A row a step offers: an object of a package or a row of the assembly.
#[derive(Clone, Debug)]
struct Choice {
    label: String,
    detail: String,
    /// Package objects: the package path and export index.
    path: Option<String>,
    index: Option<usize>,
    /// Assembly rows: the id.
    id: Option<u64>,
}

/// A texture parameter of the parent material and the image chosen for it.
#[derive(Clone, Debug)]
struct Param {
    name: String,
    current: String,
    /// The current texture's package (a path in the game folder) and export: the model of a new one.
    model: Option<(String, usize)>,
    file: Option<String>,
}

pub struct Wizard {
    backend: Backend,
    studio: WeakEntity<Studio>,
    focus: FocusHandle,
    list_focus: FocusHandle,
    kind: Option<Kind>,
    /// Highlighted kind card on Step::Choose for keyboard navigation.
    chosen_kind: Kind,
    /// Kinds offered on the first step (all, or the package or content ones).
    offer: &'static [Kind],
    step: usize,
    file: Option<String>,
    query: Entity<InputState>,
    results: Vec<Choice>,
    note: Option<String>,
    seq: u64,
    model: Option<Choice>,
    bot: Option<Choice>,
    /// Champion pre-filled from the Champions view, preserved when stepping back.
    champion_hint: Option<String>,
    name: Entity<InputState>,
    desc: Entity<InputState>,
    /// The short description of an ability, card or item.
    short: Entity<InputState>,
    slot: Entity<InputState>,
    /// The slots an ability or card can go in: valid value id and name.
    slots: Vec<(i64, String)>,
    /// The id range new content takes its ids from, as typed.
    range_start: Entity<InputState>,
    range_end: Entity<InputState>,
    range_values: (String, String),
    /// Whether the advanced custom content ID range inputs are unfolded.
    show_id_range: bool,
    /// The next ids once the range is applied, or why it is refused.
    next_ids: Option<Result<String, String>>,
    /// A skin's parts, named after the skin (each can be renamed).
    parts: Vec<SkinPart>,
    /// The template collection: its item, champion and theme (the part names are rewritten from them).
    source: Option<(u64, String, String)>,
    packages: Vec<(String, String)>,
    package: Option<String>,
    groups: Vec<(usize, String)>,
    group: Option<usize>,
    params: Vec<Param>,
    busy: Option<String>,
    error: Option<String>,
    /// The objects of the open package, offered before any search: (path, index, name, class).
    nearby: Vec<(String, usize, String, String)>,
    /// Folder where a new `.upk` package is created (`mod-files/files` in a project, else `Documents`).
    pkg_dir: Entity<InputState>,
    pkg_dir_value: String,
    /// Script packages (name, path) and the one a new class goes in.
    scripts: Vec<(String, String)>,
    script_pkg: Option<String>,
    parent: Entity<InputState>,
    starter: Starter,
    /// The open project's `mod-files/scripts`, where a Lua script goes.
    mod_scripts: Option<std::path::PathBuf>,
    _subs: Vec<Subscription>,
}

fn default_package_dir(mod_scripts: Option<&std::path::Path>) -> String {
    if let Some(files) = mod_scripts.and_then(|s| s.parent()).map(|m| m.join("files")) {
        return files.to_string_lossy().to_string();
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| std::path::Path::new(&h).join("Documents"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir)
        .to_string_lossy()
        .to_string()
}

/// A part of the skin collection being copied.
struct SkinPart {
    item: i64,
    kind: String,
    /// Its name in the template.
    template: String,
    /// The name last suggested (a part still named so follows the skin's name).
    suggested: String,
    input: Entity<InputState>,
}

/// `text` with the champion's name taken out (the rule of `rename` in `mcts/skins.rs`).
fn theme_of(text: &str, champion: &str) -> String {
    if champion.is_empty() {
        return text.to_string();
    }
    text.split(champion).collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn swap_text(text: &str, from: &str, to: &str) -> Option<String> {
    if from.is_empty() || !text.contains(from) {
        return None;
    }
    Some(text.split(from).collect::<Vec<_>>().join(to).split_whitespace().collect::<Vec<_>>().join(" "))
}

/// A part's name for a skin called `name` on `new_champion`, from its name in the template collection.
fn rename_part(text: &str, name: &str, old_champion: &str, new_champion: &str, old_theme: &str) -> String {
    let new_theme = theme_of(name.trim(), new_champion);
    let mut out = text.to_string();
    let mut changed = old_theme == new_theme;
    if old_theme != new_theme {
        if let Some(a) = swap_text(&out, old_theme, &new_theme) {
            out = a;
            changed = true;
        }
    }
    if !old_champion.is_empty() && old_champion != new_champion {
        if let Some(b) = swap_text(&out, old_champion, new_champion) {
            out = b;
            changed = true;
        }
    }
    if changed {
        out
    } else {
        format!("{new_theme} {text}").trim().to_string()
    }
}

/// The open package, its selection's group and its objects, as the wizard starts from them.
pub struct Start {
    /// The user's id range for new content (the settings').
    pub id_range: Option<(i64, i64)>,
    /// Start at the details of this kind with this template: (kind, its id, its name).
    pub preset: Option<(Kind, u64, String)>,
    pub package: Option<String>,
    pub group: Option<usize>,
    pub nearby: Vec<(String, usize, String, String)>,
    pub packages: Vec<(String, String)>,
    pub dir: String,
    pub scripts: Vec<(String, String)>,
    pub mod_scripts: Option<std::path::PathBuf>,
}

impl Wizard {
    pub fn new(backend: Backend, studio: WeakEntity<Studio>, start: Start, kind: Option<Kind>, content: Option<bool>, window: &mut Window, cx: &mut Context<Self>) -> Wizard {
        let input = |ph: &str, window: &mut Window, cx: &mut Context<Self>| {
            let ph = ph.to_string();
            cx.new(|cx| InputState::new(window, cx).placeholder(ph))
        };
        let query = input("Search by name", window, cx);
        let name = input("Name", window, cx);
        let desc = input("Description shown in game (optional)", window, cx);
        let short = input("One line under the name (optional)", window, cx);
        let slot = input("Slot: a valid value id (optional: the template's)", window, cx);
        let range_start = input("start", window, cx);
        let range_end = input("end", window, cx);
        let range_values = match start.id_range {
            Some((a, b)) => {
                let (sa, sb) = (a.to_string(), b.to_string());
                range_start.update(cx, |s, cx| s.set_value(sa.clone(), window, cx));
                range_end.update(cx, |s, cx| s.set_value(sb.clone(), window, cx));
                (sa, sb)
            }
            None => (String::new(), String::new()),
        };
        let parent = input("Object", window, cx);
        let pkg_dir_value = default_package_dir(start.mod_scripts.as_deref());
        let pkg_dir = input("Folder for the package", window, cx);
        pkg_dir.update(cx, |s, cx| s.set_value(pkg_dir_value.clone(), window, cx));
        let _ = &start.dir;
        let mut subs = vec![
            cx.subscribe(&query, |this: &mut Wizard, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.search(cx);
                }
            }),
            cx.subscribe(&pkg_dir, |this: &mut Wizard, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.pkg_dir_value = this.pkg_dir.read(cx).value().trim().to_string();
                    this.error = None;
                    cx.notify();
                }
            }),
            // The parts of a skin are named after it as it is typed.
            cx.subscribe_in(&name, window, |this: &mut Wizard, _, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::Change) {
                    this.rename_parts(window, cx);
                }
            }),
        ];
        for field in [&range_start, &range_end] {
            subs.push(cx.subscribe(field, |this: &mut Wizard, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.range_values = (
                        this.range_start.read(cx).value().trim().to_string(),
                        this.range_end.read(cx).value().trim().to_string(),
                    );
                }
                if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.range_values = (
                        this.range_start.read(cx).value().trim().to_string(),
                        this.range_end.read(cx).value().trim().to_string(),
                    );
                    this.apply_range(cx);
                }
            }));
        }
        let offer = match content {
            Some(true) => Kind::CONTENT,
            Some(false) => Kind::PACKAGE,
            None => &[],
        };
        let chosen_kind = if offer == Kind::PACKAGE { Kind::Texture } else { Kind::Skin };
        let mut w = Wizard {
            backend,
            studio,
            focus: cx.focus_handle(),
            list_focus: cx.focus_handle(),
            kind: None,
            chosen_kind,
            offer,
            step: 0,
            file: None,
            query,
            results: Vec::new(),
            note: None,
            seq: 0,
            model: None,
            bot: None,
            champion_hint: None,
            name,
            desc,
            short,
            slot,
            slots: Vec::new(),
            range_start,
            range_end,
            range_values,
            show_id_range: false,
            next_ids: None,
            parts: Vec::new(),
            source: None,
            packages: start.packages,
            package: start.package.clone(),
            groups: Vec::new(),
            group: start.group,
            params: Vec::new(),
            busy: None,
            error: None,
            nearby: start.nearby,
            pkg_dir,
            pkg_dir_value,
            script_pkg: start.scripts.iter().find(|(n, _)| n.eq_ignore_ascii_case("TgGame")).or(start.scripts.first()).map(|(_, p)| p.clone()),
            scripts: start.scripts,
            parent,
            starter: Starter::Event,
            mod_scripts: start.mod_scripts,
            _subs: subs,
        };
        if let Some(p) = start.package {
            w.load_groups(p, start.group, cx);
        }
        if let Some((k, id, label)) = start.preset {
            // A template already chosen (a skin collection to copy): straight to its details.
            w.chosen_kind = k;
            w.kind = Some(k);
            w.model = Some(Choice { label, detail: String::new(), path: None, index: None, id: Some(id) });
            w.step = k.steps().iter().position(|s| *s == Step::Details).unwrap_or(1);
            w.enter(window, cx);
        } else if let Some(k) = kind {
            w.choose(k, window, cx);
        }
        w
    }

    /// Pre-select a champion (e.g. from the Champions page's "New skin…" button) and jump straight to picking the template.
    pub(super) fn set_champion(&mut self, bot_id: u64, bot_name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.bot = Some(Choice { label: bot_name.clone(), detail: String::new(), path: None, index: None, id: Some(bot_id) });
        self.champion_hint = Some(bot_name);
        if self.current() == Step::Champion {
            self.step += 1;
            self.enter(window, cx);
        } else {
            cx.notify();
        }
    }

    /// Apply the id range as typed (kept in the settings), then read the next ids it gives.
    fn apply_range(&mut self, cx: &mut Context<Self>) {
        let (a, b) = self.range_values.clone();
        let range = if a.is_empty() && b.is_empty() {
            None
        } else {
            match (a.parse::<i64>(), b.parse::<i64>()) {
                (Ok(a), Ok(b)) if a >= 0 && b > a => Some((a, b)),
                _ => {
                    self.next_ids = Some(Err("start and end must be whole numbers with start < end".into()));
                    self.show_id_range = true;
                    cx.notify();
                    return;
                }
            }
        };
        let studio = self.studio.clone();
        cx.defer(move |cx| {
            let _ = studio.update(cx, |s, _| s.set_id_range(range));
        });
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let _ = backend.call("mcts.open", json!({})).await;
            let params = match range {
                Some((a, b)) => json!({ "start": a, "end": b }),
                None => json!({}),
            };
            let r = match backend.call("mcts.idRange", params).await {
                Ok(_) => backend.call("mcts.nextIds", json!({})).await,
                Err(e) => Err(e),
            };
            let _ = this.update(cx, |this, cx| {
                if r.is_err() {
                    this.show_id_range = true;
                }
                this.next_ids = Some(
                    r.map(|r| format!("next: item {}, bot {}, msg {}", r.json["item"], r.json["bot"], r.json["msg"]))
                        .map_err(|e| crate::studio::ui::explain_error(&e)),
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// The slots a device can go in (the valid values named ES…, Card or Talent).
    fn load_slots(&mut self, cx: &mut Context<Self>) {
        if !self.slots.is_empty() {
            return;
        }
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let _ = backend.call("mcts.open", json!({})).await;
            let Ok(r) = backend.call("mcts.validValues", json!({})).await else { return };
            let _ = this.update(cx, |this, cx| {
                let is_slot = |t: &str| {
                    let es = t.strip_prefix("ES").is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
                    let l = t.to_ascii_lowercase();
                    es || l.contains("card") || l.contains("talent")
                };
                let mut slots: Vec<(i64, String)> = r.json.as_object().into_iter().flatten().filter_map(|(id, v)| Some((id.parse().ok()?, v["text"].as_str()?.to_string()))).filter(|(_, t)| is_slot(t)).collect();
                // ES2 before ES10.
                let key = |t: &str| (t.trim_end_matches(|c: char| !c.is_ascii_digit() && c != 'S' && c != 'E').to_string(), t.to_string());
                let num = |t: &str| t.strip_prefix("ES").and_then(|r| r.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok()).unwrap_or(u32::MAX);
                slots.sort_by(|a, b| num(&a.1).cmp(&num(&b.1)).then_with(|| key(&a.1).cmp(&key(&b.1))));
                this.slots = slots;
                cx.notify();
            });
        })
        .detach();
    }

    /// The parts of the skin collection being copied, each with a name to give it.
    fn load_parts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.model.as_ref().and_then(|m| m.id) else { return };
        if self.source.as_ref().is_some_and(|s| s.0 == item) {
            return;
        }
        self.parts.clear();
        self.source = None;
        let backend = self.backend.clone();
        cx.spawn_in(window, async move |this, cx| {
            let _ = backend.call("mcts.open", json!({})).await;
            let Ok(r) = backend.call("mcts.collection", json!({ "item": item })).await else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                if this.model.as_ref().and_then(|m| m.id) != Some(item) {
                    return;
                }
                let j = &r.json;
                this.source = Some((item, j["champion"].as_str().unwrap_or_default().to_string(), j["theme"].as_str().unwrap_or_default().to_string()));
                this.parts = j["parts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| {
                        let template = p["name"].as_str().unwrap_or_default().to_string();
                        Some(SkinPart { item: p["item"].as_i64()?, kind: p["kind"].as_str().unwrap_or_default().to_string(), template, suggested: String::new(), input: cx.new(|cx| InputState::new(window, cx)) })
                    })
                    .collect();
                this.rename_parts(window, cx);
            });
        })
        .detach();
    }

    /// Name the parts after the skin: those the user has not renamed follow.
    fn rename_parts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, old_champion, old_theme)) = self.source.clone() else { return };
        let name = self.name.read(cx).value().to_string();
        let new_champion = self.bot.as_ref().map(|b| b.label.clone()).unwrap_or_else(|| old_champion.clone());
        for p in &mut self.parts {
            let next = rename_part(&p.template, &name, &old_champion, &new_champion, &old_theme);
            let untouched = p.suggested.is_empty() || p.input.read(cx).value().as_ref() == p.suggested.as_str();
            if untouched {
                p.input.update(cx, |s, cx| s.set_value(next.clone(), window, cx));
            }
            p.suggested = next;
        }
        cx.notify();
    }

    fn steps(&self) -> &'static [Step] {
        self.kind.map(Kind::steps).unwrap_or(&[Step::Choose])
    }

    fn current(&self) -> Step {
        self.steps().get(self.step).copied().unwrap_or(Step::Choose)
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        let _ = self.studio.update(cx, |s, cx| {
            s.wizard = None;
            cx.notify();
        });
    }

    fn choose(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        self.chosen_kind = kind;
        self.kind = Some(kind);
        self.model = None;
        self.bot = None;
        self.file = None;
        self.params.clear();
        self.error = None;
        self.step = 1;
        self.enter(window, cx);
    }

    /// Visual 2-column rows shown on Step::Choose, chunked into groups of ≤ 4 cards.
    fn choose_sections(&self) -> Vec<(&'static str, &'static [Kind])> {
        choose_sections_for(self.offer)
    }

    fn step_choose_kind(&mut self, key: &str, cx: &mut Context<Self>) {
        const COLS: usize = 2;
        let sections = self.choose_sections();
        let rows: Vec<Vec<Kind>> = sections.iter().flat_map(|(_, kinds)| kinds.chunks(COLS).map(|c| c.to_vec())).collect();
        let flat: Vec<Kind> = rows.iter().flatten().copied().collect();
        if flat.is_empty() {
            return;
        }
        match key {
            "home" => self.chosen_kind = flat[0],
            "end" => self.chosen_kind = *flat.last().unwrap(),
            "left" => {
                let at = flat.iter().position(|&k| k == self.chosen_kind).unwrap_or(0);
                self.chosen_kind = flat[at.saturating_sub(1)];
            }
            "right" => {
                let at = flat.iter().position(|&k| k == self.chosen_kind).unwrap_or(0);
                self.chosen_kind = flat[(at + 1).min(flat.len() - 1)];
            }
            "up" | "down" => {
                let (r, c) = rows
                    .iter()
                    .enumerate()
                    .find_map(|(r, row)| row.iter().position(|&k| k == self.chosen_kind).map(|c| (r, c)))
                    .unwrap_or((0, 0));
                let next_r = if key == "up" { r.saturating_sub(1) } else { (r + 1).min(rows.len() - 1) };
                let row = &rows[next_r];
                self.chosen_kind = row[c.min(row.len() - 1)];
            }
            _ => return,
        }
        cx.notify();
    }

    fn step_choice(&mut self, dir: isize, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            return;
        }
        let step = self.current();
        if !matches!(step, Step::Champion | Step::Model) {
            return;
        }
        let cur = if step == Step::Champion { self.bot.as_ref() } else { self.model.as_ref() };
        let at = cur.and_then(|m| self.results.iter().position(|c| c.label == m.label && c.path == m.path && c.index == m.index && c.id == m.id));
        let last = self.results.len() - 1;
        let next = match (dir, at) {
            (isize::MIN, _) => 0,
            (isize::MAX, _) => last,
            (d, Some(i)) => (i as isize + d).clamp(0, last as isize) as usize,
            (_, None) => 0,
        };
        let picked = self.results[next].clone();
        if step == Step::Champion {
            self.bot = Some(picked);
            self.model = None;
        } else {
            if self.model.as_ref().map(|m| (&m.path, m.index, m.id)) != Some((&picked.path, picked.index, picked.id)) {
                self.params.clear();
            }
            self.model = Some(picked);
        }
        cx.notify();
    }

    /// What a step does when it is shown: clear the search, fill in names.
    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        match self.current() {
            Step::Choose => {
                self.focus.focus(window, cx);
            }
            Step::Champion => {
                let initial = self.champion_hint.clone().unwrap_or_default();
                self.query.update(cx, |s, cx| s.set_value(initial.clone(), window, cx));
                self.query.update(cx, |s, cx| s.focus(window, cx));
                self.search_query(initial, cx);
            }
            Step::Model => {
                self.query.update(cx, |s, cx| s.set_value("", window, cx));
                self.query.update(cx, |s, cx| s.focus(window, cx));
                self.search_query(String::new(), cx);
            }
            Step::Place | Step::Details => {
                if self.kind == Some(Kind::UnrealClass) {
                    self.query.update(cx, |s, cx| s.set_value("", window, cx));
                }
                if self.name.read(cx).value().trim().is_empty() {
                    let suggested = self.suggest_name();
                    self.name.update(cx, |s, cx| s.set_value(suggested, window, cx));
                }
                self.name.update(cx, |s, cx| s.focus(window, cx));
                // Game content: the ids it will take, the slots it can go in, a skin's parts.
                if let Some(kind) = self.kind.filter(|k| k.content()) {
                    self.apply_range(cx);
                    if matches!(kind, Kind::Ability | Kind::Card) {
                        self.load_slots(cx);
                    }
                    if kind == Kind::Skin {
                        self.load_parts(window, cx);
                    }
                }
            }
            Step::Textures => self.load_params(cx),
            _ => {}
        }
        cx.notify();
    }

    fn suggest_name(&self) -> String {
        let Some(kind) = self.kind else { return String::new() };
        let clean = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect::<String>();
        match kind {
            Kind::Texture | Kind::StaticMesh | Kind::SkeletalMesh | Kind::Sound | Kind::Movie => {
                let stem = self.file.as_deref().and_then(|f| std::path::Path::new(f).file_stem().map(|s| s.to_string_lossy().to_string())).unwrap_or_default();
                let stem = clean(&stem);
                let p = kind.prefix();
                if p.is_empty() || stem.to_ascii_lowercase().starts_with(p) { stem } else { format!("{p}{stem}") }
            }
            Kind::MaterialInstance => {
                let base = self.model.as_ref().map(|m| m.label.rsplit('.').next().unwrap_or("").to_string()).unwrap_or_default();
                let base = base.trim_start_matches("mat_").trim_start_matches("mic_").trim_start_matches("mi_").to_string();
                format!("mic_{}_custom", clean(&base))
            }
            Kind::Copy => self.model.as_ref().map(|m| format!("{}_copy", m.label.rsplit('.').next().unwrap_or(""))).unwrap_or_default(),
            Kind::Skin => self.bot.as_ref().map(|b| format!("Custom {}", b.label)).unwrap_or_else(|| "Custom skin".into()),
            Kind::Package => "MyMod_Assets".into(),
            Kind::LuaScript => "main".into(),
            Kind::UnrealClass => "MyModClass".into(),
            _ => String::new(),
        }
    }

    // ----- searching -----------------------------------------------------------------

    fn search(&mut self, cx: &mut Context<Self>) {
        let q = self.query.read(cx).value().trim().to_string();
        self.search_query(q, cx);
    }

    fn search_query(&mut self, q: String, cx: &mut Context<Self>) {
        let Some(kind) = self.kind else { return };
        self.seq += 1;
        let seq = self.seq;
        self.note = None;
        let step = self.current();
        if !matches!(step, Step::Model | Step::Champion) {
            // The Place step filters its package list with the same field.
            cx.notify();
            return;
        }
        // Game content: the assembly's rows by name.
        if kind.content() || step == Step::Champion {
            let (what, bot) = match (step, kind) {
                (Step::Champion, _) | (_, Kind::Champion) => ("bot", None),
                (_, Kind::Skin) => ("collection", self.bot.as_ref().and_then(|b| b.id)),
                (_, Kind::Item) => ("item", None),
                _ => ("device", self.bot.as_ref().and_then(|b| b.id)),
            };
            let backend = self.backend.clone();
            self.note = Some("Reading the game's content…".into());
            cx.spawn(async move |this, cx| {
                let _ = backend.call("mcts.open", json!({})).await;
                let r = backend.call("mcts.choices", json!({ "kind": what, "bot": bot, "query": q, "limit": 300 })).await;
                let _ = this.update(cx, |this, cx| {
                    if this.seq != seq {
                        return;
                    }
                    match r {
                        Ok(r) => {
                            this.results = r.json.as_array().into_iter().flatten().map(|c| Choice { label: c["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("#{}", c["id"])), detail: choice_detail(what, c), path: None, index: None, id: c["id"].as_u64() }).collect();
                            this.note = this.results.is_empty().then(|| "Nothing matches".to_string());
                        }
                        Err(e) => this.note = Some(super::ui::explain_error(&e)),
                    }
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        let is_a = kind.class();
        if q.chars().count() < 2 {
            // Before a search: objects of the right kind in the open package.
            self.results = self
                .nearby
                .iter()
                .filter(|(_, _, _, c)| is_a.is_none_or(|k| class_fits(c, k)))
                .take(100)
                .map(|(p, i, n, c)| Choice { label: n.clone(), detail: format!("{c} · {}", file_name(p)), path: Some(p.clone()), index: Some(*i), id: None })
                .collect();
            self.note = Some(if self.results.is_empty() { "Type two letters or more to search every package".into() } else { "From the open package; type to search every package".into() });
            cx.notify();
            return;
        }
        let call = self.backend.call("workspace.search", json!({ "query": q, "limit": 150, "isA": is_a }));
        self.note = Some("Searching…".into());
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.seq != seq {
                    return;
                }
                match r {
                    Ok(r) => {
                        this.results = r.json["results"].as_array().into_iter().flatten().map(|h| Choice { label: h["object"].as_str().unwrap_or("").to_string(), detail: format!("{} · {}", h["class"].as_str().unwrap_or(""), h["package"].as_str().unwrap_or("")), path: h["path"].as_str().map(str::to_string), index: h["index"].as_u64().map(|i| i as usize), id: None }).collect();
                        this.note = if this.results.is_empty() { Some("Nothing matches".into()) } else if r.json["truncated"].as_bool() == Some(true) { Some("The first matches: type more to narrow it down".into()) } else { None };
                    }
                    Err(e) => this.note = Some(super::ui::explain_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_groups(&mut self, package: String, keep: Option<usize>, cx: &mut Context<Self>) {
        let call = self.backend.call("package.open", json!({ "path": package }));
        cx.spawn(async move |this, cx| {
            let Ok(r) = call.await else { return };
            let t = crate::model::Tables::from_json(&package, &r.json);
            let _ = this.update(cx, |this, cx| {
                if this.package.as_deref() != Some(package.as_str()) {
                    return;
                }
                let mut groups: Vec<(usize, String)> = (0..t.exports.len()).filter(|&i| t.exports[i].class.eq_ignore_ascii_case("package")).map(|i| (i, t.object_path(i).split_once('.').map(|(_, r)| r.to_string()).unwrap_or_default())).collect();
                groups.sort_by_key(|g| g.1.to_ascii_lowercase());
                this.group = keep.filter(|k| groups.iter().any(|g| g.0 == *k));
                this.groups = groups;
                cx.notify();
            });
        })
        .detach();
    }

    /// The parent material's texture parameters.
    fn load_params(&mut self, cx: &mut Context<Self>) {
        let Some((path, index)) = self.model.as_ref().and_then(|m| Some((m.path.clone()?, m.index?))) else { return };
        if !self.params.is_empty() {
            return;
        }
        let call = self.backend.call("material.preview", json!({ "path": path, "index": index }));
        let packages = self.packages.clone();
        self.note = Some("Reading the material…".into());
        cx.spawn(async move |this, cx| {
            let r = call.await;
            let _ = this.update(cx, |this, cx| {
                this.note = None;
                match r {
                    Ok(r) => {
                        this.params = r.json["textures"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|t| {
                                let name = t["parameter"].as_str()?.to_string();
                                let package = t["package"].as_str().unwrap_or("");
                                let model = packages.iter().find(|(n, _)| n.eq_ignore_ascii_case(package)).map(|(_, p)| p.clone()).zip(t["export"].as_u64().map(|e| e as usize));
                                Some(Param { name, current: t["name"].as_str().unwrap_or("").to_string(), model, file: None })
                            })
                            .collect();
                        if this.params.is_empty() {
                            this.note = Some("This material has no texture parameters: the instance can change its scalars and colours later.".into());
                        }
                    }
                    Err(e) => this.note = Some(super::ui::explain_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ----- moving between steps ------------------------------------------------------

    /// Why Next cannot go on yet.
    fn blocker(&self, cx: &App) -> Option<String> {
        let kind = self.kind?;
        let name = self.name.read(cx).value().trim().to_string();
        match self.current() {
            Step::Choose => None,
            Step::File => match &self.file {
                None => Some("Choose a file".into()),
                Some(f) => {
                    let ext = f.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                    (!kind.extensions().contains(&ext.as_str())).then(|| format!("A {} is made from a {} file", kind.title().to_lowercase(), kind.extensions().join(", ")))
                }
            },
            Step::Champion => self.bot.is_none().then(|| "Pick the champion".into()),
            Step::Model => self.model.is_none().then(|| "Pick one in the list".into()),
            Step::Textures => None,
            Step::Place => {
                if name.is_empty() {
                    Some("Give it a name".into())
                } else if name.contains(['.', ' ']) {
                    Some("Object names have no dots or spaces".into())
                } else if kind != Kind::Package && self.package.is_none() {
                    Some("Pick the package it goes in".into())
                } else {
                    None
                }
            }
            Step::Details => {
                let ident = |s: &str| s.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                let parent = self.parent.read(cx).value().trim().to_string();
                if name.is_empty() {
                    Some("Give it a name".into())
                } else if kind == Kind::LuaScript && !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                    Some("A script name is letters, digits, _ and - (it becomes <name>.lua)".into())
                } else if kind == Kind::UnrealClass && !ident(&name) {
                    Some("A class name starts with a letter and has only letters, digits and _".into())
                } else if kind == Kind::UnrealClass && !parent.is_empty() && !parent.split('.').all(ident) {
                    Some("The parent is a class name (or Package.Class)".into())
                } else if kind == Kind::UnrealClass && self.script_pkg.is_none() {
                    Some("Pick the script package it goes in".into())
                } else if kind == Kind::Package && (name.contains(['.', ' ', '/', '\\']) || self.packages.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name))) {
                    Some("A package name has no dots, spaces or slashes, and no other package has it".into())
                } else if kind == Kind::Package && self.pkg_dir_value.trim().is_empty() {
                    Some("Choose a folder for the package".into())
                } else {
                    None
                }
            }
            Step::Review => None,
        }
    }

    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current() == Step::Choose {
            self.choose(self.chosen_kind, window, cx);
            return;
        }
        if let Some(why) = self.blocker(cx) {
            self.error = Some(why);
            cx.notify();
            return;
        }
        if self.current() == Step::Review {
            self.create(cx);
            return;
        }
        self.step += 1;
        self.enter(window, cx);
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.step == 0 {
            return;
        }
        self.step -= 1;
        if self.step == 0 {
            if let Some(k) = self.kind.take() {
                self.chosen_kind = k;
            }
        }
        self.enter(window, cx);
    }

    fn pick_file(&mut self, cx: &mut Context<Self>) {
        let kind = self.kind;
        let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(kind.map(|k| format!("{} file", k.title())).unwrap_or_default().into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut p))) = files.await else { return };
            if let Some(f) = p.pop() {
                let _ = this.update(cx, |this, cx| this.set_file(f.to_string_lossy().to_string(), cx));
            }
        })
        .detach();
    }

    fn set_file(&mut self, file: String, cx: &mut Context<Self>) {
        self.file = Some(file);
        self.error = None;
        cx.notify();
    }

    // ----- making it ---------------------------------------------------------------------

    fn create(&mut self, cx: &mut Context<Self>) {
        let Some(kind) = self.kind else { return };
        let name = self.name.read(cx).value().trim().to_string();
        let outer = self.group.map(J::from).unwrap_or_else(|| J::from("top"));
        let package = self.package.clone().unwrap_or_default();
        let model = self.model.clone();
        let backend = self.backend.clone();
        self.busy = Some(format!("Making the {}…", kind.title().to_lowercase()));
        self.error = None;
        cx.notify();
        let params: Vec<Param> = self.params.iter().filter(|p| p.file.is_some()).cloned().collect();
        let desc = self.desc.read(cx).value().trim().to_string();
        let slot = self.slot.read(cx).value().trim().parse::<u64>().ok();
        let short = self.short.read(cx).value().trim().to_string();
        // The names typed for a skin's parts, by the template part's item.
        let part_names: serde_json::Map<String, serde_json::Value> = self.parts.iter().map(|p| (p.item.to_string(), json!(p.input.read(cx).value().trim()))).collect();
        let bot = self.bot.as_ref().and_then(|b| b.id);
        let file = self.file.clone();
        let pkg_dir = self.pkg_dir_value.trim().to_string();
        let (script_pkg, starter, mod_scripts) = (self.script_pkg.clone(), self.starter, self.mod_scripts.clone());
        let parent = Some(self.parent.read(cx).value().trim().to_string()).filter(|p| !p.is_empty()).unwrap_or_else(|| "Object".into());
        // A Lua script outside a project: where it goes is asked first.
        let folder = (kind == Kind::LuaScript && mod_scripts.is_none()).then(|| cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Folder for the script".into()) }));
        cx.spawn(async move |this, cx| {
            // (package path, export index, message) of a package object, or the assembly row made.
            let result: Result<Outcome, String> = async {
                let at = |m: &Option<Choice>| m.as_ref().and_then(|m| Some(json!({ "path": m.path.clone()?, "index": m.index? })));
                match kind {
                    Kind::Texture | Kind::StaticMesh | Kind::SkeletalMesh | Kind::Sound | Kind::Movie => {
                        let r = backend.call("object.add", json!({ "path": package, "file": file, "template": at(&model), "name": name, "outer": outer })).await?;
                        Ok(Outcome::Object(package.clone(), r.json["index"].as_u64().map(|i| i as usize), format!("Made {}: {}", r.json["path"].as_str().unwrap_or(""), r.json["message"].as_str().unwrap_or(""))))
                    }
                    Kind::MaterialInstance => {
                        let r = backend.call("material.newInstance", json!({ "path": package, "parent": at(&model), "name": name, "outer": outer, "unique": true })).await?;
                        let mi = r.json["index"].as_u64().unwrap_or(0) as usize;
                        let made = r.json["path"].as_str().unwrap_or("").to_string();
                        let mut done = Vec::new();
                        for p in &params {
                            let Some((mp, me)) = &p.model else { continue };
                            let tex = format!("{name}_{}", p.name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect::<String>());
                            let t = backend.call("object.add", json!({ "path": package, "file": p.file, "template": { "path": mp, "index": me }, "name": tex, "outer": outer })).await.map_err(|e| format!("{}: {e}", p.name))?;
                            let ti = t.json["index"].as_u64().unwrap_or(0);
                            backend.call("material.setTexture", json!({ "path": package, "index": mi, "name": p.name, "texture": { "path": package, "index": ti } })).await.map_err(|e| format!("{}: {e}", p.name))?;
                            done.push(p.name.clone());
                        }
                        let extra = if done.is_empty() { String::new() } else { format!(" with your {}", done.join(", ")) };
                        Ok(Outcome::Object(package.clone(), Some(mi), format!("Made {made}{extra}. Point a mesh slot at it to use it")))
                    }
                    Kind::Copy => {
                        let m = model.clone().ok_or("no object")?;
                        let r = backend.call("object.copy", json!({ "src": m.path, "index": m.index, "dst": package, "outer": outer, "deep": true })).await?;
                        let index = r.json["index"].as_u64();
                        let mut made = r.json["path"].as_str().unwrap_or("").to_string();
                        // The copy keeps the original's name unless another was given.
                        if let Some(i) = index.filter(|_| !made.rsplit('.').next().unwrap_or("").eq_ignore_ascii_case(&name)) {
                            if let Ok(rn) = backend.call("object.rename", json!({ "path": package, "index": i, "name": name })).await {
                                made = rn.json["path"].as_str().map(str::to_string).unwrap_or(made);
                            }
                        }
                        let mut msg = format!("Copied as {made}");
                        if let Some(w) = r.json["warnings"].as_array().and_then(|w| w.first()).and_then(|w| w.as_str()) {
                            msg.push_str(&format!(" ({w})"));
                        }
                        Ok(Outcome::Object(package.clone(), r.json["index"].as_u64().map(|i| i as usize), msg))
                    }
                    Kind::Group => {
                        let o = if outer.is_string() { J::Null } else { outer.clone() };
                        let r = backend.call("object.addGroup", json!({ "path": package, "name": name, "outer": o })).await?;
                        Ok(Outcome::Object(package.clone(), r.json["index"].as_u64().map(|i| i as usize), format!("Made the group {}", r.json["path"].as_str().unwrap_or(""))))
                    }
                    Kind::Package => {
                        let path = std::path::Path::new(&pkg_dir).join(format!("{name}.upk")).to_string_lossy().to_string();
                        backend.call("package.create", json!({ "path": path })).await?;
                        Ok(Outcome::Package(path))
                    }
                    Kind::LuaScript => {
                        let dir = match (mod_scripts, folder) {
                            (Some(d), _) => d,
                            (None, Some(f)) => match f.await {
                                Ok(Ok(Some(mut p))) => p.pop().ok_or("no folder chosen")?,
                                _ => return Err("no folder chosen".into()),
                            },
                            (None, None) => return Err("no folder".into()),
                        };
                        let file = dir.join(format!("{name}.lua"));
                        if file.exists() {
                            return Err(format!("{} already exists", file.display()));
                        }
                        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                        std::fs::write(&file, starter.body(&name)).map_err(|e| e.to_string())?;
                        Ok(Outcome::File(file.to_string_lossy().to_string()))
                    }
                    Kind::UnrealClass => {
                        let pkg = script_pkg.ok_or("no script package")?;
                        let source = format!("class {name} extends {parent};\n\n// Made with Tempest Studio: compiled into {}. Edit it in its Source tab, then Compile.\n\ndefaultproperties\n{{\n}}\n", file_name(&pkg));
                        let r = backend.call("script.compile", json!({ "path": pkg, "class": name, "source": source, "create": true })).await?;
                        let added = r.json["added"].as_u64().map(|i| i as usize);
                        Ok(Outcome::Script(pkg.clone(), added, format!("Made the class {name} in {}", file_name(&pkg))))
                    }
                    Kind::Ability | Kind::Card | Kind::Item | Kind::Champion | Kind::Skin => {
                        let what = match kind {
                            Kind::Ability => "device",
                            Kind::Card => "card",
                            Kind::Item => "item",
                            Kind::Champion => "champion",
                            _ => "skin",
                        };
                        let template = model.as_ref().and_then(|m| m.id).ok_or("no template")?;
                        let mut params = json!({ "kind": what, "template": template, "name": name, "description": desc, "bot": bot, "slot": slot });
                        if !short.is_empty() {
                            params["shortDescription"] = json!(short);
                        }
                        if kind == Kind::Skin && !part_names.is_empty() {
                            params["names"] = serde_json::Value::Object(part_names);
                        }
                        let r = backend.call("mcts.create", params).await?;
                        let j = &r.json;
                        let row = [&j["bot"], &j["device"], &j["item"], &j["subPath"]].into_iter().find(|p| p.is_array()).and_then(|p| p.as_array().and_then(|a| a.first()).and_then(|f| Some((f[0].as_str()?.to_string(), f[1].as_u64()? as usize))));
                        let ids = [("device", &j["deviceId"]), ("item", &j["itemId"]), ("bot", &j["botId"])].iter().filter_map(|(k, v)| v.as_i64().map(|v| format!("{k} {v}"))).collect::<Vec<_>>().join(", ");
                        Ok(Outcome::Content(row, format!("Made the {} {name} ({ids})", kind.title().to_lowercase())))
                    }
                }
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(outcome) => {
                        let _ = this.studio.update(cx, |s, cx| s.wizard_done(outcome, cx));
                    }
                    Err(e) => {
                        this.error = Some(e);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Dev: `wizard <step>` of the test driver: `next`, `back`, `file <path>`, `query <text>`, `pick <row>`, `name <text>`, `create`.
    pub fn drive(&mut self, verb: &str, rest: &str, window: &mut Window, cx: &mut Context<Self>) {
        match verb {
            "next" => self.next(window, cx),
            "back" => self.back(window, cx),
            "file" => self.set_file(rest.trim().to_string(), cx),
            "query" => {
                let q = rest.trim().to_string();
                self.query.update(cx, |s, cx| s.set_value(q.clone(), window, cx));
                self.search_query(q, cx);
            }
            "pick" => {
                if let Some(c) = rest.trim().parse::<usize>().ok().and_then(|k| self.results.get(k).cloned()) {
                    if self.current() == Step::Champion {
                        self.bot = Some(c);
                    } else {
                        self.model = Some(c);
                        self.params.clear();
                    }
                }
            }
            "name" => {
                let n = rest.trim().to_string();
                self.name.update(cx, |s, cx| s.set_value(n, window, cx));
                self.rename_parts(window, cx);
            }
            "range" => {
                let (a, b) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
                let (a, b) = (a.to_string(), b.to_string());
                self.range_values = (a.clone(), b.clone());
                self.show_id_range = true;
                self.range_start.update(cx, |s, cx| s.set_value(a, window, cx));
                self.range_end.update(cx, |s, cx| s.set_value(b, window, cx));
                self.apply_range(cx);
            }
            "dump" => println!("{}", json!({ "step": format!("{:?}", self.current()), "parts": self.parts.iter().map(|p| json!([p.kind, p.template, p.input.read(cx).value().to_string()])).collect::<Vec<_>>(), "slots": self.slots.len(), "nextIds": self.next_ids.as_ref().map(|n| n.clone().unwrap_or_else(|e| format!("error: {e}"))), "error": self.error })),
            _ => {}
        }
        cx.notify();
    }

    // ----- drawing ---------------------------------------------------------------------

    fn render_choose(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let chosen_kind = self.chosen_kind;
        let sections = self.choose_sections();
        let card = |k: Kind, cx: &mut Context<Self>| {
            let selected = chosen_kind == k;
            h_flex()
                .id(SharedString::from(format!("wiz-{k:?}")))
                .focusable()
                .tab_index(0)
                .relative()
                .min_w_0()
                // Uniform minimum height so rows align while growing cleanly with text zoom.
                .min_h(px(54.0))
                .items_start()
                .gap_3()
                .px_3()
                .py_1p5()
                .rounded_lg()
                .border_1()
                .border_color(if selected { theme.ring } else { theme.border })
                .bg(if selected { theme.secondary } else { theme.secondary.opacity(0.4) })
                .focus_visible(|s| s.border_color(theme.ring))
                .cursor_pointer()
                .hover(|s| s.border_color(theme.ring).bg(theme.secondary))
                .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                    if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.choose(k, window, cx);
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.choose(k, window, cx)))
                .child(div().flex_none().mt_0p5().p_2().rounded_md().bg(theme.primary.opacity(0.15)).text_color(theme.primary).child(Icon::new(k.icon())))
                .child(v_flex().flex_1().min_w_0().child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).overflow_hidden().text_ellipsis().whitespace_nowrap().child(k.title())).child(div().text_xs().line_clamp(2).text_color(theme.muted_foreground).child(k.blurb())))
                .child(crate::driver::probe(format!("wiz-{k:?}")))
        };
        let mut body = v_flex().gap_3();
        for (si, (title, kinds)) in sections.into_iter().enumerate() {
            for (ci, chunk) in kinds.chunks(4).enumerate() {
                let mut grid = div().grid().grid_cols(2).gap_1p5();
                for &k in chunk {
                    grid = grid.child(card(k, cx));
                }
                let mut group = v_flex().id(("wiz-sec", si * 8 + ci)).gap_1p5();
                if ci == 0 {
                    group = group.child(crate::studio::ui::section(title, &theme).pt_0().pb_0());
                }
                body = body.child(group.child(grid));
            }
        }
        body.into_any_element()
    }

    fn render_list(&mut self, cx: &mut Context<Self>, chosen: Option<&Choice>, on_pick: fn(&mut Wizard, Choice, &mut Window, &mut Context<Wizard>)) -> AnyElement {
        let theme = cx.theme().clone();
        let list_focus = self.list_focus.clone();
        let mut list = v_flex().gap_0p5();
        for (k, c) in self.results.iter().enumerate() {
            let sel = chosen.is_some_and(|m| m.label == c.label && m.path == c.path && m.index == c.index && m.id == c.id);
            let (pick, key_pick) = (c.clone(), c.clone());
            let (lf, lf_key) = (list_focus.clone(), list_focus.clone());
            list = list.child(
                h_flex()
                    .id(("wiz-row", k))
                    .focusable()
                    .relative()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(if sel { theme.ring } else { gpui::transparent_black() })
                    .focus_visible(|s| s.border_color(theme.ring))
                    .cursor_pointer()
                    .when(sel, |d| d.bg(theme.list_active))
                    .hover(|s| s.bg(theme.list_hover))
                    .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                        if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                            window.prevent_default();
                            cx.stop_propagation();
                            lf_key.focus(window, cx);
                            on_pick(this, key_pick.clone(), window, cx);
                            if e.keystroke.key == "enter" {
                                this.next(window, cx);
                            }
                        }
                    }))
                    .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                        lf.focus(window, cx);
                        on_pick(this, pick.clone(), window, cx);
                        // A double click picks and goes on.
                        if e.click_count() > 1 {
                            this.next(window, cx);
                        }
                    }))
                    .child(div().flex_1().min_w_0().text_sm().overflow_hidden().text_ellipsis().whitespace_nowrap().child(c.label.clone()))
                    .child(div().max_w(px(300.0)).text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(c.detail.clone()))
                    .child(crate::driver::probe(format!("wiz-row-{k}"))),
            );
        }
        v_flex()
            .gap_2()
            .child(Input::new(&self.query).small().cleanable(true))
            .when_some(self.note.clone(), |d, n| d.child(div().text_xs().text_color(theme.muted_foreground).child(n)))
            .child(
                div()
                    .id("wiz-list")
                    .track_focus(&list_focus)
                    .tab_index(0)
                    .h(px(300.0))
                    .rounded_md()
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(|s| s.border_color(theme.ring))
                    .overflow_y_scroll()
                    .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| match e.keystroke.key.as_str() {
                        "up" if !e.keystroke.modifiers.modified() => {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.step_choice(-1, cx);
                        }
                        "down" if !e.keystroke.modifiers.modified() => {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.step_choice(1, cx);
                        }
                        "home" if !e.keystroke.modifiers.modified() => {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.step_choice(isize::MIN, cx);
                        }
                        "end" if !e.keystroke.modifiers.modified() => {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.step_choice(isize::MAX, cx);
                        }
                        "space" if !e.keystroke.modifiers.modified() => {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.step_choice(0, cx);
                        }
                        "enter" if !e.keystroke.modifiers.modified() => {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.step_choice(0, cx);
                            this.next(window, cx);
                        }
                        _ => {}
                    }))
                    .child(list),
            )
            .into_any_element()
    }

    fn render_file(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let kind = self.kind.unwrap_or(Kind::Texture);
        let exts = kind.extensions().iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(", ");
        v_flex()
            .id("wiz-drop")
            .gap_3()
            .p_6()
            .items_center()
            .justify_center()
            .h(px(240.0))
            .rounded_lg()
            .border_2()
            .border_dashed()
            .border_color(theme.border)
            .drag_over::<ExternalPaths>(|s, _, _, cx| s.border_color(cx.theme().primary).bg(cx.theme().primary.opacity(0.08)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                if let Some(p) = paths.paths().first() {
                    this.set_file(p.to_string_lossy().to_string(), cx);
                }
            }))
            .child(Icon::new(IconName::Upload).large().text_color(theme.muted_foreground))
            .child(match &self.file {
                Some(f) => v_flex().items_center().child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(file_name(f))).child(div().text_xs().text_color(theme.muted_foreground).child(f.clone())).into_any_element(),
                None => div().text_sm().child(format!("Drop a {exts} file here")).into_any_element(),
            })
            .child(div().relative().child(Button::new("wiz-file").label(if self.file.is_some() { "Choose another…" } else { "Choose a file…" }).on_click(cx.listener(|this, _, _, cx| this.pick_file(cx)))).child(crate::driver::probe("wiz-file")))
            .into_any_element()
    }

    fn render_textures(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut list = v_flex().gap_2();
        for (k, p) in self.params.iter().enumerate() {
            let no_model = p.model.is_none();
            list = list.child(
                h_flex()
                    .gap_3()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .child(v_flex().flex_1().min_w_0().child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(p.name.clone())).child(div().text_xs().text_color(theme.muted_foreground).overflow_hidden().text_ellipsis().whitespace_nowrap().child(match &p.file {
                        Some(f) => format!("your image: {}", file_name(f)),
                        None => format!("keeps {}", p.current),
                    })))
                    .child(
                        Button::new(("wiz-tex", k))
                            .xsmall()
                            .label(if p.file.is_some() { "Change…" } else { "Use my image…" })
                            .disabled(no_model)
                            .when(no_model, |b| b.tooltip("Pick a model first"))
                            .on_click(cx.listener(move |_this, _, _, cx| {
                                let files = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Image (PNG or DDS)".into()) });
                                cx.spawn(async move |this, cx| {
                                    let Ok(Ok(Some(mut f))) = files.await else { return };
                                    if let Some(f) = f.pop() {
                                        let _ = this.update(cx, |this, cx| {
                                            if let Some(p) = this.params.get_mut(k) {
                                                p.file = Some(f.to_string_lossy().to_string());
                                            }
                                            cx.notify();
                                        });
                                    }
                                })
                                .detach();
                            })),
                    )
                    .when(p.file.is_some(), |d| {
                        d.child(Button::new(("wiz-tex-keep", k)).xsmall().ghost().label("Keep the original").on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(p) = this.params.get_mut(k) {
                                p.file = None;
                            }
                            cx.notify();
                        })))
                    }),
            );
        }
        v_flex()
            .gap_2()
            .child(div().text_xs().text_color(theme.muted_foreground).child("Each image becomes a texture next to the instance, made like the one it replaces (same compression and settings). Leave any as they are."))
            .when_some(self.note.clone(), |d, n| d.child(div().text_xs().text_color(theme.muted_foreground).child(n)))
            .child(div().id("wiz-tex-list").max_h(px(320.0)).overflow_y_scroll().child(list))
            .into_any_element()
    }

    fn render_place(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let kind = self.kind.unwrap_or(Kind::Texture);
        let mut pkgs = v_flex().gap_0p5();
        let current = self.package.clone();
        // The open and recently used packages first: a short list, the rest through the search box.
        let filter = self.query.read(cx).value().to_ascii_lowercase();
        let mut shown = 0;
        for (n, p) in &self.packages {
            if !filter.is_empty() && !n.to_ascii_lowercase().contains(&filter) && current.as_deref() != Some(p.as_str()) {
                continue;
            }
            if shown >= 60 {
                break;
            }
            shown += 1;
            let sel = current.as_deref() == Some(p.as_str());
            let (path, key_path) = (p.clone(), p.clone());
            pkgs = pkgs.child(
                div()
                    .id(SharedString::from(format!("wiz-pkg-{n}")))
                    .focusable()
                    .tab_index(0)
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .border_1()
                    .border_color(if sel { theme.ring } else { gpui::transparent_black() })
                    .focus_visible(|s| s.border_color(theme.ring))
                    .text_sm()
                    .cursor_pointer()
                    .when(sel, |d| d.bg(theme.list_active))
                    .hover(|s| s.bg(theme.list_hover))
                    .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                        if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.package = Some(key_path.clone());
                            this.groups.clear();
                            this.group = None;
                            this.load_groups(key_path.clone(), None, cx);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.package = Some(path.clone());
                        this.groups.clear();
                        this.group = None;
                        this.load_groups(path.clone(), None, cx);
                        cx.notify();
                    }))
                    .child(n.clone()),
            );
        }
        let mut groups = h_flex().flex_wrap().gap_1();
        let top = {
            let b = Button::new("wiz-group-top").xsmall().label("top level").on_click(cx.listener(|this, _, _, cx| {
                this.group = None;
                cx.notify();
            }));
            b.ghost().selected(self.group.is_none())
        };
        groups = groups.child(top);
        for (i, g) in self.groups.iter().take(40) {
            let i = *i;
            let b = Button::new(("wiz-group", i)).xsmall().label(g.clone()).on_click(cx.listener(move |this, _, _, cx| {
                this.group = Some(i);
                cx.notify();
            }));
            groups = groups.child(b.ghost().selected(self.group == Some(i)));
        }
        let name_hint = match kind.prefix() {
            "" => "no dots or spaces".to_string(),
            p => format!("no dots or spaces; the game's own start with {p}"),
        };
        v_flex()
            .gap_3()
            .child(v_flex().gap_1().child(label("Name", &theme)).child(Input::new(&self.name)).child(div().text_xs().text_color(theme.muted_foreground).child(name_hint)))
            .child(
                v_flex()
                    .gap_1()
                    .child(label("Package", &theme))
                    .child(Input::new(&self.query).small().cleanable(true))
                    .child(div().id("wiz-pkgs").h(px(150.0)).overflow_y_scroll().rounded_md().border_1().border_color(theme.border).p_1().child(pkgs))
                    .child(div().text_xs().text_color(theme.muted_foreground).child("Your own package (Create ▸ Package) keeps your work apart from the game's; a game package works too when the object must be in it.")),
            )
            .child(v_flex().gap_1().child(label("Group", &theme)).child(div().id("wiz-groups").max_h(px(90.0)).overflow_y_scroll().child(groups)))
            .into_any_element()
    }

    fn render_details(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let kind = self.kind.unwrap_or(Kind::Item);
        if kind == Kind::LuaScript {
            let mut starters = h_flex().gap_1().flex_wrap();
            for s in Starter::ALL {
                let b = Button::new(SharedString::from(format!("wiz-starter-{s:?}"))).small().label(s.title()).on_click(cx.listener(move |this, _, _, cx| {
                    this.starter = s;
                    cx.notify();
                }));
                starters = starters.child(b.ghost().selected(self.starter == s));
            }
            let name = self.name.read(cx).value().trim().to_string();
            let where_ = match &self.mod_scripts {
                Some(d) => format!("In the project: {}", d.join(format!("{name}.lua")).display()),
                None => "No project is open: Create asks for a folder. In a project it goes in mod-files/scripts/ and ships with the mod.".into(),
            };
            return v_flex()
                .gap_3()
                .child(v_flex().gap_1().child(label("File name", &theme)).child(Input::new(&self.name)).child(div().text_xs().text_color(theme.muted_foreground).child("becomes <name>.lua; scripts of a mod run in name order")))
                .child(v_flex().gap_1().child(label("Starts with", &theme)).child(starters))
                .child(div().p_2().rounded_md().bg(theme.secondary).text_xs().font_family(crate::studio::ui::mono()).child(self.starter.body(if name.is_empty() { "main" } else { &name })))
                .child(div().text_xs().text_color(theme.muted_foreground).child(where_))
                .into_any_element();
        }
        if kind == Kind::UnrealClass {
            let filter = self.query.read(cx).value().to_ascii_lowercase();
            let mut pkgs = v_flex().gap_0p5();
            for (n, p) in self.scripts.iter().filter(|(n, _)| filter.is_empty() || n.to_ascii_lowercase().contains(&filter)) {
                let sel = self.script_pkg.as_deref() == Some(p.as_str());
                let (path, key_path) = (p.clone(), p.clone());
                pkgs = pkgs.child(
                    div()
                        .id(SharedString::from(format!("wiz-spkg-{n}")))
                        .focusable()
                        .tab_index(0)
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .border_1()
                        .border_color(if sel { theme.ring } else { gpui::transparent_black() })
                        .focus_visible(|s| s.border_color(theme.ring))
                        .text_sm()
                        .cursor_pointer()
                        .when(sel, |d| d.bg(theme.list_active))
                        .hover(|s| s.bg(theme.list_hover))
                        .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                            if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.script_pkg = Some(key_path.clone());
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.script_pkg = Some(path.clone());
                            cx.notify();
                        }))
                        .child(n.clone()),
                );
            }
            return v_flex()
                .gap_3()
                .child(v_flex().gap_1().child(label("Class name", &theme)).child(Input::new(&self.name)))
                .child(v_flex().gap_1().child(label("Extends", &theme)).child(Input::new(&self.parent).small()).child(div().text_xs().text_color(theme.muted_foreground).child("The class it builds on: Object, Actor, TgDevice… (empty: Object)")))
                .child(v_flex().gap_1().child(label("Script package", &theme)).child(Input::new(&self.query).small().cleanable(true)).child(div().id("wiz-spkgs").h(px(150.0)).overflow_y_scroll().rounded_md().border_1().border_color(theme.border).p_1().child(pkgs)))
                .child(div().text_xs().text_color(theme.muted_foreground).child("The package is rebuilt from its sources with the new class added (the package must have no other pending changes). Its Source tab opens next."))
                .into_any_element();
        }
        if kind == Kind::Package {
            return v_flex()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(label("Package name", &theme))
                        .child(Input::new(&self.name))
                        .child(crate::studio::ui::hint("Becomes <name>.upk, with a game package's settings. Add textures, meshes and materials to it with Create.", &theme)),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(label("Folder", &theme))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().flex_1().min_w_0().child(Input::new(&self.pkg_dir).small()))
                                .child(Button::new("wiz-pkg-dir").small().label("Browse…").on_click(cx.listener(|_this, _, window, cx| {
                                    let dirs = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Folder for the new package".into()) });
                                    let handle = window.window_handle();
                                    cx.spawn(async move |this, cx| {
                                        let Ok(Ok(Some(mut p))) = dirs.await else { return };
                                        let Some(dir) = p.pop() else { return };
                                        let text = dir.to_string_lossy().to_string();
                                        let _ = cx.update_window(handle, |_, window, cx| {
                                            let _ = this.update(cx, |this, cx| {
                                                this.pkg_dir_value = text.clone();
                                                this.pkg_dir.update(cx, |s, cx| s.set_value(text, window, cx));
                                                this.error = None;
                                                cx.notify();
                                            });
                                        });
                                    })
                                    .detach();
                                }))),
                        )
                        .child(crate::studio::ui::hint("Kept in your mod and shipped whole when you build; existing game files are never modified.", &theme)),
                )
                .into_any_element();
        }
        let (ra, rb) = &self.range_values;
        let range_summary = if !ra.is_empty() && !rb.is_empty() {
            format!("Content IDs: assigned automatically ({ra}–{rb})")
        } else {
            "Content IDs: assigned automatically (800000–899999)".to_string()
        };
        let id_section: AnyElement = if self.show_id_range {
            v_flex()
                .gap_1()
                .p_2()
                .rounded_md()
                .bg(theme.secondary)
                .child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("Advanced: custom content ID range"))
                        .child(div().flex_1())
                        .child(Button::new("wiz-id-hide").xsmall().ghost().label("Hide").on_click(cx.listener(|this, _, _, cx| {
                            this.show_id_range = false;
                            cx.notify();
                        }))),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .child(div().w(px(110.0)).child(Input::new(&self.range_start).xsmall()))
                        .child(div().text_color(theme.muted_foreground).child("…"))
                        .child(div().w(px(110.0)).child(Input::new(&self.range_end).xsmall()))
                        .child(div().flex_1())
                        .when_some(self.next_ids.as_ref().and_then(|n| n.as_ref().ok()), |d, n| d.child(div().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(n.clone()))),
                )
                .when_some(self.next_ids.as_ref().and_then(|n| n.as_ref().err()), |d, e| d.child(div().text_xs().text_color(theme.danger).child(e.clone())))
                .child(div().text_xs().text_color(theme.muted_foreground).child("Ids of new rows and messages come from this range. Give every mod its own (say 100000 wide) so mods never allocate the same id; empty: past the largest id in use."))
                .into_any_element()
        } else {
            h_flex()
                .gap_2()
                .pt_1()
                .text_xs()
                .child(div().flex_1().min_w_0().text_color(theme.muted_foreground).child(range_summary))
                .child(
                    Button::new("wiz-id-change")
                        .xsmall()
                        .ghost()
                        .label("Change…")
                        .tooltip("Advanced: custom content ID range")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_id_range = true;
                            cx.notify();
                        })),
                )
                .into_any_element()
        };
        v_flex()
            .gap_3()
            .child(v_flex().gap_1().child(label("Name in game", &theme)).child(Input::new(&self.name)).child(div().text_xs().text_color(theme.muted_foreground).child(if kind == Kind::Skin { "The skin's name; its parts are named after it (\"<name> Head\", \"<name> Weapon\"…)" } else { "A new message in every language" })))
            .when(kind == Kind::Skin && !self.parts.is_empty(), |d| {
                let mut list = v_flex().gap_1().child(label("Parts (copied from the template; their names follow yours, edit any)", &theme));
                for p in &self.parts {
                    let tip = format!("Template: {} ({})", p.template, p.item);
                    list = list.child(
                        h_flex()
                            .gap_2()
                            .child(div().id(gpui::SharedString::from(format!("part-{}", p.item))).w(px(84.0)).flex_none().px_1().rounded_sm().bg(theme.secondary).text_xs().tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)).child(p.kind.clone()))
                            .child(div().flex_1().min_w_0().child(Input::new(&p.input).xsmall())),
                    );
                }
                d.child(list)
            })
            .when(matches!(kind, Kind::Ability | Kind::Card | Kind::Item), |d| d.child(v_flex().gap_1().child(label("Short description", &theme)).child(Input::new(&self.short).small())))
            .when(kind != Kind::Skin, |d| d.child(v_flex().gap_1().child(label("Description", &theme)).child(Input::new(&self.desc))))
            .when(matches!(kind, Kind::Ability | Kind::Card), |d| {
                // The slot is a valid value: picked by name, or typed as its id.
                let chosen = self.slot.read(cx).value().trim().parse::<i64>().ok().and_then(|id| self.slots.iter().find(|s| s.0 == id)).map(|s| s.1.clone());
                let (slots, me) = (self.slots.clone(), cx.entity().downgrade());
                let slots_empty = slots.is_empty();
                let pick = Button::new("wiz-slot")
                    .small()
                    .label("Pick…")
                    .disabled(slots_empty)
                    .when(slots_empty, |b| b.tooltip("No equip slots available for this champion"))
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu.scrollable(true).max_h(px(360.0));
                        for (id, text) in &slots {
                            let (me, id) = (me.clone(), *id);
                            menu = menu.item(gpui_component::menu::PopupMenuItem::new(format!("{text}  ({id})")).on_click(move |_, window, cx| {
                                let _ = me.update(cx, |this, cx| {
                                    this.slot.update(cx, |s, cx| s.set_value(id.to_string(), window, cx));
                                    cx.notify();
                                });
                            }));
                        }
                        menu
                    });
                d.child(v_flex().gap_1().child(label("Slot", &theme)).child(h_flex().gap_2().child(div().flex_1().min_w_0().child(Input::new(&self.slot).small())).child(pick)).child(div().text_xs().text_color(theme.muted_foreground).child(chosen.unwrap_or_else(|| "Empty: the template's slot".into()))))
            })
            .child(id_section)
            .into_any_element()
    }

    fn render_review(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let kind = self.kind.unwrap_or(Kind::Texture);
        let name = self.name.read(cx).value().trim().to_string();
        let pkg = self.package.as_deref().map(file_name).unwrap_or_default();
        let group = self.group.and_then(|g| self.groups.iter().find(|x| x.0 == g)).map(|g| g.1.clone());
        let place = match &group {
            Some(g) => format!("{pkg}, group {g}"),
            None => format!("{pkg}, top level"),
        };
        let model = self.model.as_ref().map(|m| format!("{} ({})", m.label, m.detail)).unwrap_or_default();
        let mut lines: Vec<(&str, String)> = Vec::new();
        match kind {
            Kind::Texture | Kind::StaticMesh | Kind::SkeletalMesh | Kind::Sound | Kind::Movie => {
                lines.push(("From", self.file.as_deref().map(file_name).unwrap_or_default()));
                lines.push(("Made like", model));
                lines.push(("Name", name));
                lines.push(("In", place));
            }
            Kind::MaterialInstance => {
                lines.push(("Based on", model));
                let mine: Vec<String> = self.params.iter().filter_map(|p| p.file.as_ref().map(|f| format!("{} ← {}", p.name, file_name(f)))).collect();
                lines.push(("Your textures", if mine.is_empty() { "none yet (it draws like its parent)".into() } else { mine.join(", ") }));
                lines.push(("Name", name));
                lines.push(("In", place));
            }
            Kind::Copy => {
                lines.push(("Copy of", model));
                lines.push(("Name", name));
                lines.push(("In", place));
            }
            Kind::Group => {
                lines.push(("Group", name));
                lines.push(("In", place));
            }
            Kind::Package => {
                lines.push(("Package", format!("{name}.upk")));
                lines.push(("Folder", self.pkg_dir_value.clone()));
            }
            Kind::LuaScript => {
                lines.push(("File", format!("{name}.lua")));
                lines.push(("Starts with", self.starter.title().to_string()));
                lines.push(("In", self.mod_scripts.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "a folder you pick next".into())));
            }
            Kind::UnrealClass => {
                let parent = self.parent.read(cx).value().trim().to_string();
                lines.push(("Class", format!("{name} extends {}", if parent.is_empty() { "Object" } else { &parent })));
                lines.push(("In", self.script_pkg.as_deref().map(file_name).unwrap_or_default()));
            }
            _ => {
                if let Some(b) = &self.bot {
                    lines.push(("Champion", b.label.clone()));
                }
                lines.push(("Copied from", model));
                lines.push(("Name", name));
                let d = self.desc.read(cx).value().trim().to_string();
                if !d.is_empty() {
                    lines.push(("Description", d));
                }
            }
        }
        let mut list = v_flex().gap_2();
        for (k, v) in lines {
            list = list.child(h_flex().gap_3().items_start().child(div().w(px(110.0)).flex_none().text_xs().text_color(theme.muted_foreground).child(k)).child(div().flex_1().min_w_0().text_sm().child(v)));
        }
        v_flex()
            .gap_3()
            .child(list)
            .child(div().text_xs().text_color(theme.muted_foreground).child(if kind == Kind::LuaScript {
                "A file on disk; open it in your code editor."
            } else if kind.content() {
                "It is a change to the game's data: Undo takes it back, and Build mod ships it as a delta (adds only your new rows without replacing the whole game data file)."
            } else {
                "It is a change like any other: Undo takes it back, and Build mod puts it into your mod."
            }))
            .into_any_element()
    }
}

fn label(text: &'static str, theme: &gpui_component::Theme) -> impl IntoElement {
    div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(text)
}

/// Whether an object of `class` can model a `kind` (search results already are; this filters the open package's).
fn class_fits(class: &str, kind: &str) -> bool {
    let c = class.to_ascii_lowercase();
    match kind {
        "MaterialInterface" => c == "material" || c.starts_with("materialinstance"),
        "Texture2D" => c == "texture2d",
        k => c == k.to_ascii_lowercase(),
    }
}

/// What a wizard made.
pub enum Outcome {
    /// An object: its package, export and the message.
    Object(String, Option<usize>, String),
    Package(String),
    /// Game content: the row to show (table, index) and the message.
    Content(Option<(String, usize)>, String),
    /// A new class: its package, export and the message (its Source tab opens).
    Script(String, Option<usize>, String),
    /// A file written to disk.
    File(String),
}

impl Render for Wizard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let step = self.current();
        let steps = self.steps();
        let kind = self.kind;
        let title = match kind {
            Some(k) => format!("New {}", k.title().to_lowercase()),
            None => "Create".to_string(),
        };
        let body = match step {
            Step::Choose => self.render_choose(cx),
            Step::File => self.render_file(cx),
            Step::Champion => {
                let chosen = self.bot.clone();
                self.render_list(cx, chosen.as_ref(), |this, c, _, cx| {
                    this.bot = Some(c);
                    this.model = None;
                    cx.notify();
                })
            }
            Step::Model => {
                let chosen = self.model.clone();
                self.render_list(cx, chosen.as_ref(), |this, c, _, cx| {
                    if this.model.as_ref().map(|m| (&m.path, m.index, m.id)) != Some((&c.path, c.index, c.id)) {
                        this.params.clear();
                    }
                    this.model = Some(c);
                    cx.notify();
                })
            }
            Step::Textures => self.render_textures(cx),
            Step::Place => self.render_place(cx),
            Step::Details => self.render_details(cx),
            Step::Review => self.render_review(cx),
        };
        let hint: Option<String> = match (step, kind) {
            (Step::Choose, _) => Some("Pick what to make with a click, or use the arrow keys and press Enter.".to_string()),
            (Step::Model, Some(Kind::Skin)) if self.bot.is_some() => {
                let who = self.bot.as_ref().map(|b| b.label.as_str()).unwrap_or("this champion");
                Some(format!("For {who}. {}", Kind::Skin.model_hint()))
            }
            (Step::Model, Some(k)) => Some(k.model_hint().to_string()),
            (Step::Champion, Some(Kind::Skin)) => Some("Whose skin is it? Their collections are offered next.".to_string()),
            (Step::Champion, _) => Some("Whose ability, card or item is it? Their abilities, cards and talents are offered next.".to_string()),
            _ => None,
        };
        // The rail: every step of this kind, the current one marked.
        let mut rail = v_flex().w(px(170.0)).flex_none().gap_1().py_1();
        for (k, s) in steps.iter().enumerate() {
            let state = if k < self.step { 0 } else if k == self.step { 1 } else { 2 };
            rail = rail.child(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .text_color(if state == 2 { theme.muted_foreground } else { theme.foreground })
                    .child(
                        div()
                            .size(px(20.0))
                            .flex_none()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .when(state == 1, |d| d.bg(theme.primary).text_color(theme.primary_foreground))
                            .when(state == 0, |d| d.bg(theme.success.opacity(0.14)).text_color(theme.success))
                            .when(state == 2, |d| d.border_1().border_color(theme.border))
                            .child(if state == 0 {
                                Icon::new(IconName::Check).xsmall().into_any_element()
                            } else {
                                (k + 1).to_string().into_any_element()
                            }),
                    )
                    .child(s.title()),
            );
        }
        let last = step == Step::Review;
        let busy = self.busy.clone();
        let blocker = self.blocker(cx);
        let blocked = blocker.is_some();
        let why_disabled = if busy.is_some() { Some("Creating…".to_string()) } else { blocker.clone() };
        // What went wrong, else what is still needed to go on.
        let (note, bad) = match (&self.error, &blocker) {
            (Some(e), _) => (super::ui::explain_error(e), true),
            (None, Some(b)) if step != Step::Choose => (b.clone(), false),
            (None, _) if step == Step::Choose => (format!("Selected: {}", self.chosen_kind.title()), false),
            _ => (String::new(), false),
        };
        let footer = h_flex()
            .gap_2()
            .child(div().flex_1().min_w_0().text_xs().text_color(if bad { theme.danger } else { theme.muted_foreground }).child(note))
            .child(
                div()
                    .relative()
                    .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                        if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.close(cx);
                        }
                    }))
                    .child(Button::new("wiz-cancel").ghost().label("Cancel").on_click(cx.listener(|this, _, _, cx| this.close(cx))))
                    .child(crate::driver::probe("wiz-cancel")),
            )
            .when(self.step > 0, |d| {
                d.child(
                    div()
                        .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                            if (e.keystroke.key == "enter" || e.keystroke.key == "space") && !e.keystroke.modifiers.modified() && this.busy.is_none() {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.back(window, cx);
                            }
                        }))
                        .child(
                            Button::new("wiz-back")
                                .label("Back")
                                .disabled(busy.is_some())
                                .when(busy.is_some(), |b| b.tooltip("Creating…"))
                                .on_click(cx.listener(|this, _, window, cx| this.back(window, cx))),
                        ),
                )
            })
            .child(
                div()
                    .relative()
                    .child(
                        Button::new("wiz-next")
                            .primary()
                            .label(busy.clone().unwrap_or_else(|| if last { "Create".into() } else { "Next".into() }))
                            .disabled(busy.is_some() || blocked)
                            .when_some(why_disabled, |b, why| b.tooltip(why))
                            .on_click(cx.listener(|this, _, window, cx| this.next(window, cx))),
                    )
                    .child(crate::driver::probe("wiz-next")),
            );
        div()
            .id("wizard-backdrop")
            .absolute()
            .inset_0()
            // Nothing behind takes clicks or scrolling while it is open.
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.overlay)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    if this.current() == Step::Choose && this.busy.is_none() {
                        this.close(cx);
                    }
                }),
            )
            .child(
                v_flex()
                    .id("wizard")
                    .track_focus(&self.focus)
                    .w(px(880.0))
                    .max_w(gpui::relative(0.94))
                    .h(px(720.0))
                    .max_h(gpui::relative(0.90))
                    .bg(theme.popover)
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| match e.keystroke.key.as_str() {
                        "escape" => this.close(cx),
                        "enter" => this.next(window, cx),
                        "space" if this.current() == Step::Choose && !e.keystroke.modifiers.modified() => {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.choose(this.chosen_kind, window, cx);
                        }
                        k @ ("left" | "right" | "up" | "down" | "home" | "end") if this.current() == Step::Choose => {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.focus.focus(window, cx);
                            this.step_choose_kind(k, cx);
                        }
                        "up" if matches!(this.current(), Step::Champion | Step::Model) => {
                            cx.stop_propagation();
                            this.step_choice(-1, cx);
                        }
                        "down" if matches!(this.current(), Step::Champion | Step::Model) => {
                            cx.stop_propagation();
                            this.step_choice(1, cx);
                        }
                        _ => {}
                    }))
                    .child(h_flex().px_5().pt_4().pb_2().gap_2().child(Icon::new(kind.map(Kind::icon).unwrap_or(IconName::Plus)).text_color(theme.primary)).child(div().text_lg().font_weight(gpui::FontWeight::SEMIBOLD).child(title)))
                    .child(
                        h_flex()
                            .items_start()
                            .flex_1()
                            .min_h_0()
                            .px_5()
                            .gap_5()
                            .when(kind.is_some(), |d| d.child(rail))
                            .child(
                                v_flex()
                                    .id("wiz-body")
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_y_scroll()
                                    .gap_3()
                                    .pb_2()
                                    .when_some(hint, |d, h| d.child(div().text_sm().text_color(theme.muted_foreground).child(h)))
                                    .child(body),
                            ),
                    )
                    .child(div().px_5().py_3().border_t_1().border_color(theme.border).child(footer)),
            )
    }
}

impl Studio {
    /// Open Create, at the first step or at `kind`; `content` narrows the first step.
    pub(crate) fn open_wizard(&mut self, kind: Option<Kind>, content: Option<bool>, window: &mut Window, cx: &mut Context<Self>) {
        self.open_wizard_with(kind, content, None, window, cx);
    }

    /// Open Create for a specific champion (e.g. "New skin…" on a champion's page), pre-selecting the champion.
    pub(super) fn open_wizard_for_champion(&mut self, kind: Kind, bot_id: u64, bot_name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_wizard(Some(kind), None, window, cx);
        if let Some(w) = self.wizard.clone() {
            w.update(cx, |w, cx| w.set_champion(bot_id, bot_name, window, cx));
        }
    }

    /// The id range new assembly content takes its ids from (kept in the settings).
    pub(crate) fn set_id_range(&mut self, range: Option<(i64, i64)>) {
        if self.prefs.id_range != range {
            self.prefs.id_range = range;
            self.prefs.save();
        }
    }

    /// Open Create; `preset` starts at the details of a kind with its template chosen.
    pub(crate) fn open_wizard_with(&mut self, kind: Option<Kind>, content: Option<bool>, preset: Option<(Kind, u64, String)>, window: &mut Window, cx: &mut Context<Self>) {
        if self.dir.is_none() {
            self.toast(Tone::Info, "Open the game folder first", cx);
            return;
        }
        let nearby = self.open.as_ref().map(|t| (0..t.exports.len()).filter(|&i| !t.exports[i].class.eq_ignore_ascii_case("package")).map(|i| (t.path.clone(), i, t.exports[i].name.clone(), t.exports[i].class.clone())).collect()).unwrap_or_default();
        let group = self.open.as_ref().zip(self.selected_export).and_then(|(t, i)| if t.exports[i].class.eq_ignore_ascii_case("package") { Some(i) } else { t.parent[i] });
        let mut packages: Vec<(String, String)> = self.packages.iter().filter(|p| !p.script).map(|p| (p.name.clone(), p.path.clone())).collect();
        // The open, pinned and recent packages lead.
        let lead: Vec<String> = self.open.iter().map(|t| t.path.clone()).chain(self.pinned()).chain(self.recent_packages()).collect();
        packages.sort_by_key(|(_, p)| lead.iter().position(|l| l == p).unwrap_or(usize::MAX));
        let scripts = self.packages.iter().filter(|p| p.script).map(|p| (p.name.clone(), p.path.clone())).collect();
        let mod_scripts = self.project.as_ref().map(|p| p.mod_files().join("scripts"));
        let start = Start { id_range: self.prefs.id_range, preset, package: self.open.as_ref().filter(|t| !self.packages.iter().any(|p| p.path == t.path && p.script)).map(|t| t.path.clone()), group, nearby, packages, dir: self.dir.clone().unwrap_or_default(), scripts, mod_scripts };
        let me = cx.entity().downgrade();
        let backend = self.backend.clone();
        let w = cx.new(|cx| Wizard::new(backend, me, start, kind, content, window, cx));
        let focus = w.read(cx).focus.clone();
        focus.focus(window, cx);
        self.wizard = Some(w);
        cx.notify();
    }

    /// The wizard made something: show it.
    pub(crate) fn wizard_done(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        self.wizard = None;
        match outcome {
            Outcome::Object(path, index, msg) => {
                self.dirty.insert(path.clone());
                self.toast(Tone::Success, msg.clone(), cx);
                let place = Place::package(path.clone(), index);
                if self.open.as_ref().is_some_and(|t| t.path == path) {
                    self.refresh_package(path, index, cx);
                    self.record(place);
                } else {
                    self.record(place.clone());
                    self.go_later(place, cx);
                }
            }
            Outcome::Package(path) => {
                let call = self.backend.call("workspace.packages", json!({}));
                cx.spawn(async move |this, cx| {
                    let Ok(list) = call.await else { return };
                    let _ = this.update(cx, |this, cx| {
                        let mut pkgs = crate::model::packages(&list.json);
                        if !pkgs.iter().any(|p| p.path == path) {
                            let name = std::path::Path::new(&path)
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| file_name(&path));
                            pkgs.push(crate::model::PackageEntry {
                                name,
                                path: path.clone(),
                                size: 0,
                                script: false,
                                map: false,
                                folder: None,
                            });
                            pkgs.sort_by_key(|p| p.name.to_ascii_lowercase());
                        }
                        this.packages = pkgs;
                        this.refilter_packages(cx);
                        this.dirty.insert(path.clone());
                        this.toast(Tone::Success, format!("{} created: add objects to it with Create", file_name(&path)), cx);
                        this.user_open_package(path, cx);
                    });
                })
                .detach();
            }
            Outcome::Script(path, index, msg) => {
                self.dirty.insert(path.clone());
                self.toast(Tone::Success, msg.clone(), cx);
                self.tab = super::inspector::Tab::Source;
                let place = Place::package(path.clone(), index);
                if self.open.as_ref().is_some_and(|t| t.path == path) {
                    self.refresh_package(path, index, cx);
                    self.record(place);
                } else {
                    self.record(place.clone());
                    self.go_later(place, cx);
                }
            }
            Outcome::File(file) => {
                self.toast(Tone::Success, format!("Wrote {file}"), cx);
                self.reveal_file(&file, cx);
            }
            Outcome::Content(row, msg) => {
                self.toast(Tone::Success, msg.clone(), cx);
                if let Some(a) = &self.assembly {
                    a.update(cx, |a, cx| a.reopen(cx));
                }
                self.applied_docs = true;
                let place = Place::Assembly { file: "assembly".into(), path: Vec::new(), table: row.as_ref().map(|r| r.0.clone()), row: row.map(|r| r.1) };
                self.record(place.clone());
                self.go_later(place, cx);
            }
        }
        cx.notify();
    }
}

const CHOOSE_SECTIONS: &[(&str, &[Kind])] = &[
    ("Game data & skins", Kind::CHOOSE_DATA),
    ("Champions, audio & UI", Kind::CHOOSE_CHAMP_MEDIA),
    ("3D models & textures", Kind::CHOOSE_MODELS),
    ("Packages & code", Kind::CHOOSE_CODE_PKGS),
];

fn choose_sections_for(offer: &'static [Kind]) -> Vec<(&'static str, &'static [Kind])> {
    if offer.is_empty() {
        CHOOSE_SECTIONS.to_vec()
    } else if offer == Kind::CONTENT {
        vec![
            ("Skins & champions", Kind::CONTENT_CHAMPS),
            ("Abilities, cards & items", Kind::CONTENT_GAMEPLAY),
        ]
    } else if offer == Kind::PACKAGE {
        vec![
            ("3D models & textures", Kind::CHOOSE_MODELS),
            ("Audio, UI & copies", Kind::PACKAGE_MEDIA_COPY),
            ("Packages & groups", Kind::PACKAGE_CONTAINERS),
        ]
    } else if offer == Kind::SCRIPTS {
        vec![("Packages & code", Kind::SCRIPTS)]
    } else {
        offer.chunks(4).map(|chunk| ("What to make", chunk)).collect()
    }
}

/// The second line of a picked row: a champion's internal name when it differs
/// from the shown one; for other rows their kind and id.
fn choice_detail(what: &str, c: &serde_json::Value) -> String {
    let detail = c["detail"].as_str().unwrap_or("");
    if what == "bot" {
        let reference = detail.strip_prefix("champion · ").or_else(|| detail.strip_prefix("bot · ")).unwrap_or(detail);
        return if reference.eq_ignore_ascii_case(c["name"].as_str().unwrap_or("")) { String::new() } else { reference.to_string() };
    }
    format!("{detail} · id {}", c["id"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_sections_chunk_to_at_most_four() {
        assert_eq!(CHOOSE_SECTIONS.len(), 4);
        let all = choose_sections_for(&[]);
        assert_eq!(all, CHOOSE_SECTIONS);
        let mut flat = Vec::new();
        for (_, kinds) in &all {
            assert!(kinds.len() <= 4);
            for k in *kinds {
                assert!(!flat.contains(k));
                flat.push(*k);
            }
        }
        assert_eq!(flat.len(), 16);
        for (_, kinds) in choose_sections_for(Kind::CONTENT) {
            assert!(kinds.len() <= 4);
        }
        for (_, kinds) in choose_sections_for(Kind::PACKAGE) {
            assert!(kinds.len() <= 4);
        }
        for (_, kinds) in choose_sections_for(Kind::SCRIPTS) {
            assert!(kinds.len() <= 4);
        }
    }

    #[test]
    fn new_package_defaults_outside_cooked_game_dir() {
        let proj = std::path::Path::new("/tmp/my-mod/mod-files/scripts");
        let files = std::path::Path::new("/tmp/my-mod/mod-files").join("files");
        assert_eq!(default_package_dir(Some(proj)), files.to_string_lossy());
        assert!(!default_package_dir(None).to_ascii_lowercase().contains("cookedpcconsole"));
    }
}
