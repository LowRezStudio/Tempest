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
    /// The ids new assembly content takes: a range of the user's own, so mods never allocate the same id.
    pub id_range: Option<(i64, i64)>,
    /// Draw game shaders from the Direct3D 11 (shader model 5, the default) or Direct3D 9 (shader model 3) caches.
    pub sm3: bool,
    /// The Studio draws its own title bar (the header, with the window buttons on Windows and Linux); off: the system's. Read when a window opens.
    pub custom_title_bar: bool,
}

fn map_panes() -> [f32; 2] {
    [280.0, 360.0]
}

fn map_hidden() -> Vec<String> {
    ["blocking", "navigation", "info"].map(String::from).to_vec()
}

fn movie_panes() -> [f32; 3] {
    [200.0, 260.0, 220.0]
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs { theme: "dark".into(), ask_in_place: true, restore: true, folders: BTreeMap::new(), projects: Vec::new(), zoom: 1.0, check_updates: true, movie_panes: movie_panes(), map_hidden: map_hidden(), map_panes: map_panes(), panes: BTreeMap::new(), id_range: None, sm3: false, custom_title_bar: true }
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

#[cfg(test)]
mod tests {
    use super::{Prefs, CATPPUCCIN};

    fn parse_rgba(hex: &str) -> (f64, f64, f64, f64) {
        let h = hex.trim_start_matches('#');
        let r = f64::from(u8::from_str_radix(&h[0..2], 16).unwrap()) / 255.0;
        let g = f64::from(u8::from_str_radix(&h[2..4], 16).unwrap()) / 255.0;
        let b = f64::from(u8::from_str_radix(&h[4..6], 16).unwrap()) / 255.0;
        let a = if h.len() == 8 { f64::from(u8::from_str_radix(&h[6..8], 16).unwrap()) / 255.0 } else { 1.0 };
        (r, g, b, a)
    }

    fn blend(fg: (f64, f64, f64, f64), bg: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
        (fg.0 * fg.3 + bg.0 * (1.0 - fg.3), fg.1 * fg.3 + bg.1 * (1.0 - fg.3), fg.2 * fg.3 + bg.2 * (1.0 - fg.3), 1.0)
    }

    fn lum(c: (f64, f64, f64, f64)) -> f64 {
        let ch = |v: f64| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2)
    }

    fn contrast(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> f64 {
        let (l1, l2) = (lum(a), lum(b));
        (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05)
    }

    #[test]
    fn default_movie_panes_leave_stage_room() {
        assert_eq!(Prefs::default().movie_panes, [200.0, 260.0, 220.0]);
    }

    #[test]
    fn bundled_themes_meet_wcag_aa() {
        let doc: serde_json::Value = serde_json::from_str(CATPPUCCIN).expect("valid catppuccin.json");
        let themes = doc["themes"].as_array().expect("themes array");
        assert_eq!(themes.len(), 2);
        for theme in themes {
            let name = theme["name"].as_str().unwrap();
            assert_eq!(theme["is_default"].as_bool(), Some(name == "Catppuccin Mocha"), "{name} is_default");
            let c = &theme["colors"];
            let hex = |k: &str| parse_rgba(c[k].as_str().unwrap());
            let (crust, mantle, base, surf0) = (hex("title_bar.background"), hex("sidebar.background"), hex("background"), hex("secondary.background"));
            let popover = hex("popover.background");
            let la_mantle = blend(hex("list.active.background"), mantle);
            let la_base = blend(hex("list.active.background"), base);
            let sel_sidebar = blend(hex("selection.background"), mantle);
            let sel_popover = blend(hex("selection.background"), popover);
            let sel_base = blend(hex("selection.background"), base);
            assert!(contrast(hex("primary.foreground"), hex("primary.background")) >= 4.5, "{name} primary button");
            for key in ["foreground", "muted.foreground", "tab.foreground", "table.head.foreground", "description_list_label.foreground"] {
                let fg = hex(key);
                for (sname, s) in [
                    ("crust", crust),
                    ("mantle", mantle),
                    ("base", base),
                    ("surface0", surf0),
                    ("list.active@mantle", la_mantle),
                    ("list.active@base", la_base),
                    ("selection@sidebar", sel_sidebar),
                    ("selection@popover", sel_popover),
                    ("selection@base", sel_base),
                ] {
                    assert!(contrast(fg, s) >= 4.5, "{name} {key} on {sname}");
                }
            }
            for tone in ["base.yellow", "base.green", "base.cyan", "base.blue", "base.magenta", "base.red"] {
                let tc = hex(tone);
                assert!(contrast(tc, base) >= 4.5 && contrast(tc, mantle) >= 4.5, "{name} {tone} on surfaces");
                assert!(contrast(tc, hex("primary.foreground")) >= 4.5, "{name} {tone} mod_icon");
            }
        }
    }
}
