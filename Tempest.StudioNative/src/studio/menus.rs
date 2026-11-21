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
            Cmd::CommandList => Box::new(CommandList),
            Cmd::GoToPackage => Box::new(GoToPackage),
            Cmd::GoToObject => Box::new(GoToObject),
            Cmd::SearchAll => Box::new(SearchAll),
            Cmd::FindInView => Box::new(FindInView),
            Cmd::Shortcuts => Box::new(Shortcuts),
            Cmd::QuickGuide => Box::new(QuickGuide),
            Cmd::ShowHome => Box::new(ShowHome),
            Cmd::ShowChampions => Box::new(ShowChampions),
            Cmd::ShowPackages => Box::new(ShowPackages),
            Cmd::ShowAssembly => Box::new(ShowAssembly),
            Cmd::ShowMod => Box::new(ShowMod),
            Cmd::BuildAndInstall => Box::new(BuildAndInstall),
            Cmd::ShowAudio => Box::new(ShowAudio),
            Cmd::ShowMap => Box::new(ShowMap),
            Cmd::AsmNewRow => Box::new(NewRow),
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

fn submenu(name: &'static str, items: Vec<MenuItem>) -> MenuItem {
    MenuItem::submenu(Menu { name: name.into(), items, disabled: false })
}

/// The native menu bar: the app menu plus seven standard menus (`File`, `Edit`,
/// `View`, `Go`, `Mod`, `Window`, `Help`), with domain tools tucked cleanly
/// into submenus of `Mod` (`Game files`, `Game data`, `Maps`).
pub fn set(cx: &mut App) {
    let menus = vec![
        Menu {
            name: "Tempest Studio".into(),
            items: vec![
                item(Cmd::Settings),
                item(Cmd::CheckUpdates),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Quit Tempest Studio", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "File".into(),
            items: vec![
                item(Cmd::OpenFolder),
                item(Cmd::OpenFile),
                item(Cmd::OpenProject),
                item(Cmd::OpenWorkspaceFile),
                MenuItem::separator(),
                item(Cmd::Save),
                item(Cmd::SaveAs),
                item(Cmd::SaveWorkspace),
                item(Cmd::SaveWorkspacePortable),
                MenuItem::separator(),
                item(Cmd::WriteToGame),
                item(Cmd::SaveAll),
            ],
            disabled: false,
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                item(Cmd::Undo),
                item(Cmd::Redo),
                MenuItem::separator(),
                item(Cmd::Create),
                item(Cmd::Rename),
                item(Cmd::Duplicate),
                item(Cmd::Delete),
                item(Cmd::CopyPath),
                MenuItem::separator(),
                item(Cmd::FindInView),
                item(Cmd::FindReplace),
                MenuItem::separator(),
                item(Cmd::Discard),
                item(Cmd::DiscardAll),
            ],
            disabled: false,
        },
        Menu {
            name: "View".into(),
            items: vec![
                item(Cmd::ShowHome),
                item(Cmd::ShowChampions),
                item(Cmd::ShowPackages),
                item(Cmd::ShowAssembly),
                item(Cmd::ShowAudio),
                item(Cmd::ShowMap),
                item(Cmd::ShowMod),
                MenuItem::separator(),
                item(Cmd::HideSide),
                item(Cmd::HideSecond),
                MenuItem::separator(),
                item(Cmd::ZoomIn),
                item(Cmd::ZoomOut),
                item(Cmd::ZoomReset),
                MenuItem::separator(),
                item(Cmd::ToggleTheme),
                item(Cmd::GameShaders),
                item(Cmd::ShaderModel),
                item(Cmd::SoundNames),
            ],
            disabled: false,
        },
        Menu {
            name: "Go".into(),
            items: vec![
                item(Cmd::Commands),
                item(Cmd::CommandList),
                MenuItem::separator(),
                item(Cmd::GoToPackage),
                item(Cmd::GoToObject),
                item(Cmd::SearchAll),
                MenuItem::separator(),
                item(Cmd::Back),
                item(Cmd::Forward),
            ],
            disabled: false,
        },
        Menu {
            name: "Mod".into(),
            items: vec![
                item(Cmd::BuildAndInstall),
                item(Cmd::ShowMod),
                MenuItem::separator(),
                item(Cmd::NewProject),
                item(Cmd::OpenProject),
                item(Cmd::ProjectFolder),
                item(Cmd::CloseProject),
                MenuItem::separator(),
                submenu(
                    "Game files",
                    vec![
                        item(Cmd::NewPackage),
                        item(Cmd::AddObject),
                        item(Cmd::AddGroup),
                        item(Cmd::BatchImport),
                        item(Cmd::PackageInfo),
                        item(Cmd::Verify),
                        item(Cmd::Rewrite),
                        item(Cmd::ExportSources),
                        item(Cmd::ModFolder),
                        MenuItem::separator(),
                        item(Cmd::ExportObject),
                        item(Cmd::ReplaceObject),
                        item(Cmd::ReplaceWith),
                        item(Cmd::CopyTo),
                        item(Cmd::ExtractRaw),
                        MenuItem::separator(),
                        item(Cmd::NewInstance),
                        item(Cmd::OwnMaterials),
                    ],
                ),
                submenu(
                    "Game data",
                    vec![
                        item(Cmd::AsmNewRow),
                        item(Cmd::AsmNewContent),
                        item(Cmd::AsmVendor),
                        item(Cmd::AsmSkins),
                        MenuItem::separator(),
                        item(Cmd::AsmReload),
                        item(Cmd::AsmExportMod),
                    ],
                ),
                submenu(
                    "Maps",
                    vec![
                        item(Cmd::MapAddActor),
                        item(Cmd::MapCollision),
                        item(Cmd::MapBakeLighting),
                        MenuItem::separator(),
                        item(Cmd::MapNewMatinee),
                        item(Cmd::MapOpenMatinee),
                        item(Cmd::MapMatineeKey),
                    ],
                ),
            ],
            disabled: false,
        },
        Menu {
            name: "Window".into(),
            items: vec![
                item(Cmd::NewWindow),
                item(Cmd::NewTab),
                MenuItem::separator(),
                item(Cmd::NextTab),
                item(Cmd::PrevTab),
                item(Cmd::CloseTab),
            ],
            disabled: false,
        },
        Menu {
            name: "Help".into(),
            items: vec![
                item(Cmd::QuickGuide),
                item(Cmd::Shortcuts),
                MenuItem::separator(),
                item(Cmd::McpServer),
            ],
            disabled: false,
        },
    ];
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
