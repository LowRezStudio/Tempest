//! What the Studio remembers between runs (`settings.json` next to the
//! recent folders): the theme, whether saving into the game folder asks
//! first, and per game folder the pinned packages, the packages opened
//! lately and where the user was when the Studio closed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Folder {
    /// Package paths shown at the top of the list.
    pub pinned: Vec<String>,
    /// Package paths opened lately, newest first.
    pub recent: Vec<String>,
    /// Where the user was: the view, the package and the export.
    pub view: Option<String>,
    pub package: Option<String>,
    pub export: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// `dark`, `light` or `system`.
    pub theme: String,
    /// Ask before writing into the game folder.
    pub ask_in_place: bool,
    /// Reopen the package and object of the last session.
    pub restore: bool,
    pub folders: BTreeMap<String, Folder>,
    /// Project files opened lately, newest first.
    pub projects: Vec<String>,
    /// Text size, 1 = the default 16 px.
    pub zoom: f32,
    /// Look for a new version a few seconds after start.
    pub check_updates: bool,
    /// The movie editor's panes: resource tree and properties widths, timeline height.
    #[serde(default = "movie_panes")]
    pub movie_panes: [f32; 3],
    /// The map view's hidden actor categories (collision and navigation clutter the view).
    #[serde(default = "map_hidden")]
    pub map_hidden: Vec<String>,
    /// The map view's side bar and inspector widths.
    #[serde(default = "map_panes")]
    pub map_panes: [f32; 2],
    /// The other views' pane widths by group (`main`, `assembly`, `audio`).
    pub panes: BTreeMap<String, Vec<f32>>,
}

fn map_panes() -> [f32; 2] {
    [280.0, 360.0]
}

fn map_hidden() -> Vec<String> {
    ["blocking", "navigation", "info"].map(String::from).to_vec()
}

fn movie_panes() -> [f32; 3] {
    [230.0, 290.0, 260.0]
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs { theme: "dark".into(), ask_in_place: true, restore: true, folders: BTreeMap::new(), projects: Vec::new(), zoom: 1.0, check_updates: true, movie_panes: movie_panes(), map_hidden: map_hidden(), map_panes: map_panes(), panes: BTreeMap::new() }
    }
}

impl Prefs {
    pub fn load() -> Prefs {
        super::state_file("settings.json").and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(f) = super::state_file("settings.json") else { return };
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(f, s);
        }
    }

    pub fn folder(&self, dir: &str) -> Folder {
        self.folders.get(dir).cloned().unwrap_or_default()
    }

    pub fn folder_mut(&mut self, dir: &str) -> &mut Folder {
        self.folders.entry(dir.to_string()).or_default()
    }
}

/// The Catppuccin palettes the Electron Studio and the launcher use (Mocha dark, Latte light).
const CATPPUCCIN: &str = include_str!("../../themes/catppuccin.json");

pub fn theme_mode(theme: &str, window: Option<&mut gpui::Window>, cx: &mut gpui::App) {
    use gpui_component::{Theme, ThemeMode, ThemeRegistry};
    let registry = ThemeRegistry::global_mut(cx);
    if !registry.themes().contains_key("Catppuccin Mocha") {
        registry.load_themes_from_str(CATPPUCCIN).expect("the bundled theme parses");
    }
    let find = |name: &str, cx: &gpui::App| ThemeRegistry::global(cx).themes().get(name).cloned();
    let (dark, light) = (find("Catppuccin Mocha", cx), find("Catppuccin Latte", cx));
    // `change` makes the global theme first; then the palettes are swapped in and applied.
    Theme::change(ThemeMode::Dark, None, cx);
    if let (Some(dark), Some(light)) = (dark, light) {
        let t = Theme::global_mut(cx);
        t.dark_theme = dark;
        t.light_theme = light;
    }
    match theme {
        "light" => Theme::change(ThemeMode::Light, window, cx),
        "system" => Theme::sync_system_appearance(window, cx),
        _ => Theme::change(ThemeMode::Dark, window, cx),
    }
    apply_zoom(Prefs::load().zoom, cx);
}

/// The text size: the root's rem follows the theme's font size.
pub fn apply_zoom(zoom: f32, cx: &mut gpui::App) {
    let z = if zoom > 0.0 { zoom.clamp(0.7, 1.6) } else { 1.0 };
    gpui_component::Theme::update(cx, |t| {
        t.font_size = gpui::px(16.0 * z);
        t.mono_font_size = gpui::px(13.0 * z);
    });
}
