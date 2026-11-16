//! The native menu bar, built from the command table: every command is in
//! the menu of its group, with its shortcut when it has one. Quit keeps
//! every window's work first.

use gpui::{Action, App, Menu, MenuItem, SystemMenuType};

use super::commands::Cmd;
use super::*;

/// Run the command at this index of `Cmd::ALL` (the menu items without an action of their own).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = studio, no_json)]
pub struct RunCmd {
    pub index: usize,
}

actions!(studio, [Quit]);

impl Cmd {
    /// The action a menu item dispatches: the one bound to the command's
    /// keys when there is one (so the menu shows them), else `RunCmd`.
    fn action(self) -> Box<dyn Action> {
        match self {
            Cmd::Undo => Box::new(Undo),
            Cmd::Redo => Box::new(Redo),
            Cmd::Save => Box::new(Save),
            Cmd::SaveAs => Box::new(SaveAs),
            Cmd::OpenFile => Box::new(OpenFile),
            Cmd::NewWindow => Box::new(NewWindow),
            Cmd::NewTab => Box::new(NewTab),
            Cmd::CloseTab => Box::new(CloseTab),
            Cmd::NextTab => Box::new(NextTab),
            Cmd::PrevTab => Box::new(PrevTab),
            Cmd::Back => Box::new(Back),
            Cmd::Forward => Box::new(Forward),
            Cmd::Commands => Box::new(Commands),
            Cmd::GoToPackage => Box::new(GoToPackage),
            Cmd::GoToObject => Box::new(GoToObject),
            Cmd::SearchAll => Box::new(SearchAll),
            Cmd::Shortcuts => Box::new(Shortcuts),
            Cmd::ShowHome => Box::new(ShowHome),
            Cmd::ShowChampions => Box::new(ShowChampions),
            Cmd::ShowPackages => Box::new(ShowPackages),
            Cmd::ShowAssembly => Box::new(ShowAssembly),
            Cmd::ShowMod => Box::new(ShowMod),
            Cmd::ShowAudio => Box::new(ShowAudio),
            Cmd::ShowMap => Box::new(ShowMap),
            Cmd::Rename => Box::new(Rename),
            Cmd::Duplicate => Box::new(Duplicate),
            Cmd::Delete => Box::new(Delete),
            Cmd::SaveWorkspace => Box::new(SaveWorkspace),
            Cmd::OpenWorkspaceFile => Box::new(OpenWorkspaceFile),
            Cmd::Settings => Box::new(Settings),
            Cmd::CopyPath => Box::new(CopyPath),
            Cmd::Create => Box::new(Create),
            Cmd::ZoomIn => Box::new(ZoomIn),
            Cmd::ZoomOut => Box::new(ZoomOut),
            Cmd::ZoomReset => Box::new(ZoomReset),
            Cmd::HideSide => Box::new(ToggleSide),
            Cmd::HideSecond => Box::new(ToggleSecond),
            Cmd::FindReplace => Box::new(FindReplace),
            c => Box::new(RunCmd { index: Cmd::ALL.iter().position(|x| *x == c).unwrap_or(0) }),
        }
    }
}

fn item(cmd: Cmd) -> MenuItem {
    MenuItem::Action { name: cmd.info().title.into(), action: cmd.action(), os_action: None, checked: false, disabled: false }
}

/// The menu bar: the app menu, then one menu per command group.
pub fn set(cx: &mut App) {
    // (group, menu name), in menu bar order.
    const MENUS: [(&str, &str); 11] = [("File", "File"), ("Edit", "Edit"), ("View", "View"), ("Go", "Go"), ("Mod", "Mod"), ("Package", "Package"), ("Object", "Object"), ("Material", "Material"), ("Assembly", "Game data"), ("Map", "Map"), ("Help", "Help")];
    let app_items = [Cmd::Settings, Cmd::CheckUpdates];
    let mut menus = vec![Menu {
        name: "Tempest Studio".into(),
        items: vec![item(Cmd::Settings), item(Cmd::CheckUpdates), MenuItem::separator(), MenuItem::os_submenu("Services", SystemMenuType::Services), MenuItem::separator(), MenuItem::action("Quit Tempest Studio", Quit)],
        disabled: false,
    }];
    for (group, name) in MENUS {
        let items: Vec<MenuItem> = Cmd::ALL.iter().copied().filter(|c| c.info().group == group && !app_items.contains(c)).map(item).collect();
        if !items.is_empty() {
            menus.push(Menu { name: name.into(), items, disabled: false });
        }
    }
    cx.set_menus(menus);
    cx.on_action(|_: &Quit, cx| quit(cx));
}

/// Keep every window's work (a named mod's file, an unnamed one's draft), then quit.
pub fn quit(cx: &mut App) {
    let saves = updates::studios(cx).into_iter().filter_map(|s| s.update(cx, |s, cx| s.keep_for_restart(cx))).collect::<Vec<_>>();
    cx.spawn(async move |cx| {
        for r in futures::future::join_all(saves).await {
            if let Err(e) = r {
                cx.update(|cx| {
                    if let Some(s) = updates::studios(cx).into_iter().next() {
                        s.update(cx, |s, cx| s.toast(Tone::Error, format!("The Studio did not quit: your changes could not be kept ({e})"), cx));
                    }
                });
                return;
            }
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}
