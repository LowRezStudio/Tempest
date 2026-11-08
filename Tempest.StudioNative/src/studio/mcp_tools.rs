//! What the MCP server's tools do in the window (`crate::mcp`): the same
//! actions as the UI, so the window follows an agent.

use gpui::{Context, Window};
use gpui_component::WindowExt;
use serde_json::{json, Value as J};

use super::{Cmd, Place, Studio, View};

impl Studio {
    pub(crate) fn mcp_tool(&mut self, tool: &str, args: &J, window: &mut Window, cx: &mut Context<Self>) -> Result<J, String> {
        let s = |k: &str| args[k].as_str().map(str::to_string);
        match tool {
            "studio_state" => Ok(self.mcp_state(cx)),
            // The picture is taken on the MCP thread (`mcp::screenshot`), from the window's id.
            "studio_screenshot" => crate::screenshot::window_id(window).map(|id| json!({ "window": id })).ok_or_else(|| "this system gives no window id".to_string()),
            "studio_swf_open" => self.swf_tool_open(args, window, cx),
            "studio_swf_act" => self.swf_tool_act(args, window, cx),
            "studio_map_act" => {
                self.set_view(View::Map, window, cx);
                let m = self.map_entity().ok_or("the map view is not open")?;
                m.update(cx, |m, cx| m.mcp_act(args, cx))
            }
            "studio_map_read" => {
                let m = self.map_entity().ok_or("the map view is not open")?;
                Ok(m.read(cx).mcp_read(args["what"].as_str().unwrap_or(""), cx))
            }
            "studio_swf_read" => self.swf_tool_read(&args["what"], &args["args"], cx),
            "studio_open_workspace" => {
                self.open_workspace(s("dir").ok_or("missing dir")?, cx);
                Ok(json!({ "status": "opening" }))
            }
            "studio_open_package" => {
                let want = s("package").ok_or("missing package")?;
                let p = self
                    .packages
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(&want) || super::file_name(&p.path).eq_ignore_ascii_case(&want) || p.path == want)
                    .map(|p| p.path.clone())
                    .ok_or_else(|| format!("no package {want} in this folder"))?;
                self.user_open_package(p.clone(), cx);
                Ok(json!({ "status": "opening", "path": p }))
            }
            "studio_select" => {
                let t = self.open.as_ref().ok_or("no package is open")?;
                let i = match (args["index"].as_u64(), s("name")) {
                    (Some(i), _) => i as usize,
                    (None, Some(n)) => (0..t.exports.len()).find(|&i| t.exports[i].name.eq_ignore_ascii_case(&n) || t.object_path(i).eq_ignore_ascii_case(&n)).ok_or_else(|| format!("no object {n}"))?,
                    _ => return Err("give index or name".into()),
                };
                if i >= t.exports.len() {
                    return Err(format!("export {i} is out of range"));
                }
                let path = t.object_path(i);
                let class = t.exports[i].class.clone();
                self.reveal(i, cx);
                self.user_select_export(i, cx);
                Ok(json!({ "index": i, "path": path, "class": class }))
            }
            "studio_set_mode" => {
                let v = match s("mode").as_deref() {
                    Some("assembly") => View::Assembly,
                    Some("audio") => View::Audio,
                    Some("mod") => View::Mod,
                    Some("packages") => View::Packages,
                    Some("map") => View::Map,
                    m => return Err(format!("unknown mode {m:?}")),
                };
                self.set_view(v, window, cx);
                Ok(json!({ "status": "ok" }))
            }
            "studio_commands" => Ok(J::Array(
                Cmd::ALL
                    .iter()
                    .map(|c| {
                        let i = c.info();
                        json!({ "id": c.id(), "title": i.title, "group": i.group, "keys": i.keys, "detail": i.detail, "enabled": c.enabled(self) })
                    })
                    .collect(),
            )),
            "studio_run_command" => {
                let id = s("id").ok_or("missing id")?;
                let c = Cmd::ALL.iter().copied().find(|c| c.id() == id).ok_or_else(|| format!("no command {id}"))?;
                if !c.enabled(self) {
                    return Err(format!("{id} cannot run now"));
                }
                self.run(c, window, cx);
                Ok(json!({ "status": "ran", "note": "a command that asks something opened a dialog for the user" }))
            }
            "studio_open_table" => {
                let table = s("table").ok_or("missing table")?;
                let place = Place::Assembly { file: s("doc").unwrap_or_else(|| "assembly".into()), table: Some(table), row: args["row"].as_u64().map(|r| r as usize) };
                self.record(place.clone());
                self.go(place, window, cx);
                Ok(json!({ "status": "ok" }))
            }
            "studio_close_dialog" => {
                window.close_dialog(cx);
                self.wizard = None;
                self.close_palette(window, cx);
                Ok(json!({ "status": "closed" }))
            }
            "studio_project" => match s("action").as_deref() {
                Some("save") => {
                    self.save_project(cx);
                    Ok(json!({ "status": "saving" }))
                }
                Some("close") => {
                    self.close_project_now(true, None, cx);
                    Ok(json!({ "status": "closing" }))
                }
                _ => Ok(json!({ "project": self.project.as_ref().map(|p| json!({ "file": p.file, "name": p.name, "id": p.id, "version": p.version, "files": p.files })), "state": self.project_saving.label() })),
            },
            "studio_mod" => {
                self.set_view(View::Mod, window, cx);
                let c = self.cooker_entity().ok_or("the Mod view is not open")?;
                match s("action").as_deref() {
                    Some("refresh") => c.update(cx, |c, cx| c.refresh(cx)),
                    Some("build") => c.update(cx, |c, cx| c.build_now(cx)),
                    _ => {}
                }
                Ok(c.read(cx).summary(cx))
            }
            t => Err(format!("unknown tool {t}")),
        }
    }

    fn mcp_state(&self, cx: &gpui::App) -> J {
        let view = match self.view {
            View::Packages => "packages",
            View::Assembly => "assembly",
            View::Audio => "audio",
            View::Mod => "mod",
            View::Map => "map",
        };
        let open = self.open.as_ref().map(|t| {
            let sel = self.selected_export.filter(|&i| i < t.exports.len()).map(|i| json!({ "index": i, "path": t.object_path(i), "class": t.exports[i].class }));
            json!({ "path": t.path, "name": t.name, "exports": t.exports.len(), "selected": sel })
        });
        let mut unsaved: Vec<String> = self.dirty.iter().cloned().collect();
        unsaved.extend(self.assembly.as_ref().map(|a| a.read(cx).unsaved_files()).unwrap_or_default());
        unsaved.sort();
        json!({
            "folder": self.dir,
            "packages": self.packages.len(),
            "view": view,
            "open": open,
            "assembly": self.assembly.as_ref().map(|a| match a.read(cx).place() { Place::Assembly { file, table, row } => json!({ "file": file, "table": table, "row": row }), _ => J::Null }),
            "unsaved": unsaved,
            "project": self.project.as_ref().map(|p| json!({ "name": p.name, "file": p.file, "state": self.project_saving.label() })),
            "wizardOpen": self.wizard.is_some(),
            "particles": self.particles_state(),
            "movie": self.shown_movie().map(|m| m.read(cx).state()),
            "map": self.map.as_ref().filter(|_| self.view == View::Map).map(|m| m.read(cx).summary(cx)),
            "status": self.status.to_string(),
        })
    }
}
