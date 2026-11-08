//! The MCP server: Streamable HTTP (JSON responses) on
//! `127.0.0.1:1423/mcp` (`TEMPEST_STUDIO_MCP_PORT`), a bearer token made at
//! start and written with the URL to `<config>/tempest-studio-native/mcp.json`.
//! `upk_call` goes straight to the backend; every other tool runs on the UI
//! thread through a channel the Studio drains (`studio::Studio::mcp_tool`), so
//! the window follows what an agent does.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use base64::Engine;
use futures::channel::{mpsc, oneshot};
use serde_json::{json, Value as J};

use crate::backend::Backend;

/// A tool call for the UI thread, and where its answer goes.
pub struct UiCall {
    pub tool: String,
    pub args: J,
    pub reply: oneshot::Sender<Result<J, String>>,
}

pub struct Mcp {
    pub url: String,
    pub token: String,
}

/// The properties of `studio_swf` (the same as the Electron Studio's).
const SWF_PROPS: &str = r#"{ "action": { "type": "string", "enum": ["movies", "open", "status", "tree", "timeline", "info", "script", "setScript", "edit", "remove", "revert", "text", "layers", "apply", "discard", "addImage", "replaceImage", "exportImage", "character", "shapeStyle", "shapeSvg", "editText", "staticText", "addGlyphs", "export", "compile"] }, "path": { "type": "string" }, "index": { "type": "integer" }, "object": { "type": "string" }, "query": { "type": "string" }, "sprite": { "type": "integer" }, "frame": { "type": "integer" }, "node": { "type": "string", "enum": ["movie", "frames", "texts", "scripts", "images", "linkage", "tags"] }, "character": { "type": "integer" }, "depth": { "type": "integer" }, "kind": { "type": "string" }, "section": { "type": "string", "enum": ["texts", "scripts", "images", "linkage", "tags"] }, "tag": { "type": "integer" }, "key": { "type": "string" }, "source": { "type": "string" }, "pcode": { "type": "string" }, "text": { "type": "string" }, "limit": { "type": "integer" }, "offset": { "type": "integer" }, "x": { "type": "number" }, "y": { "type": "number" }, "dx": { "type": "number" }, "dy": { "type": "number" }, "scaleX": { "type": "number" }, "scaleY": { "type": "number" }, "rotation": { "type": "number" }, "skew": { "type": "number" }, "matrix": { "type": "object" }, "alpha": { "type": "number" }, "name": { "type": "string" }, "hide": { "type": "array", "items": { "type": "integer" } }, "show": { "type": "array", "items": { "type": "integer" } }, "solo": { "type": "array", "items": { "type": "integer" } }, "showAll": { "type": "boolean" }, "file": { "type": "string" }, "texture": { "type": "string" }, "shape": { "type": "boolean" }, "place": { "type": "boolean" }, "cxform": { "type": ["object", "null"] }, "styleKey": { "type": "string" }, "value": { "type": "string" }, "svg": { "type": "string" }, "fit": { "type": "boolean" }, "props": { "type": "object" }, "record": { "type": "integer" }, "color": { "type": "string" }, "chars": { "type": "string" }, "format": { "type": "string", "enum": ["svg", "png", "ttf", "sound", "binary"] }, "scale": { "type": "integer" } }"#;

/// The properties of `studio_map` (the same as the Electron Studio's).
const MAP_PROPS: &str = r#"{ "action": { "type": "string", "enum": ["list", "open", "levels", "actors", "select", "move", "show", "kismet", "link", "unlink", "palette", "addNode", "removeNode", "actorPalette", "addActor", "removeActor", "heavyWalls", "canBaseOn", "export", "matinee", "setKey", "addKey", "removeKey", "matineeTime", "setInterpMode", "setTangents", "keyAtPlayhead", "matinees", "newMatinee", "addGroup", "removeGroup", "setGroup", "addTrack", "removeTrack", "setProps", "connect", "navmesh", "undo", "collision", "brush", "setBrush", "bake", "lightInfo"] },
 "path": { "type": "string" }, "level": { "type": "string" }, "export": { "type": "integer" }, "category": { "type": "string" }, "filter": { "type": "string" },
 "location": { "type": "array", "items": { "type": "number" } }, "rotation": { "type": "array", "items": { "type": "number" } },
 "categories": { "type": "object" }, "levels": { "type": "object" }, "shading": { "type": "string", "enum": ["lit", "unlit", "untextured", "wireframe"] },
 "sequence": { "type": "integer" }, "from": { "type": "integer" }, "to": { "type": "integer" }, "output": { "type": "integer" }, "input": { "type": "integer" }, "variable": { "type": "integer" },
 "nodeClass": { "type": "string" }, "actorClass": { "type": "string" }, "newClass": { "type": "string" }, "mesh": { "type": "object" }, "classes": { "type": "boolean" },
 "x": { "type": "number" }, "y": { "type": "number" }, "originator": { "type": "integer" }, "object": { "type": "integer" }, "node": { "type": "integer" }, "on": { "type": "boolean" },
 "value": {}, "out": { "type": "string" }, "skip": { "type": "array", "items": { "type": "string" } }, "volumes": { "type": "boolean" }, "textures": { "type": "boolean" }, "textureSize": { "type": "integer" },
 "track": { "type": "integer" }, "curve": { "type": "string" }, "key": { "type": "integer" }, "time": { "type": "number" }, "mode": { "type": "string" }, "arrive": {}, "leave": {}, "fields": { "type": "object" }, "values": { "type": "object" },
 "group": { "type": "integer" }, "name": { "type": "string" }, "color": { "type": "array", "items": { "type": "number" } }, "director": { "type": "boolean" }, "trackClass": { "type": "string" }, "property": { "type": "string" }, "props": { "type": "object" },
 "actor": { "type": "integer" }, "remove": { "type": "boolean" }, "length": { "type": "number" }, "brush": { "type": "object" }, "only": { "type": "boolean" },
 "scope": { "type": "string", "enum": ["selected", "edited", "level"] }, "shadowSamples": { "type": "integer" }, "skySamples": { "type": "integer" }, "indirectRays": { "type": "integer" }, "albedo": { "type": "number" }, "environmentScale": { "type": "number" }, "compare": { "type": "boolean" },
 "navOp": { "type": "string", "enum": ["status", "show", "hide", "pick", "deleteFaces", "cutRegion", "moveVertex"] }, "faces": { "type": "array", "items": { "type": "integer" } }, "vertex": { "type": "integer" }, "colour": { "type": "string", "enum": ["region", "cluster"] } }"#;

/// The tools: name, description, input schema.
fn tools() -> J {
    let obj = |props: J, required: &[&str]| json!({ "type": "object", "properties": props, "required": required });
    json!([
        { "name": "studio_state", "description": "What the Studio shows: the game folder, the view, the open package and selected object, unsaved files, the open project.", "inputSchema": obj(json!({}), &[]) },
        { "name": "upk_call", "description": "Call any backend method of `upk serve` (serve.rs: package.open, object.get, object.set, mcts.rows, …) with its params; binary payloads come back as base64. Edits made this way are unsaved, like the UI's (the Studio marks the package when it next reads it).", "inputSchema": obj(json!({ "method": { "type": "string" }, "params": { "type": "object" } }), &["method"]) },
        { "name": "studio_open_workspace", "description": "Open a game folder (the game's or its ChaosGame/CookedPCConsole).", "inputSchema": obj(json!({ "dir": { "type": "string" } }), &["dir"]) },
        { "name": "studio_open_package", "description": "Open a package of the folder by name (e.g. MAEVE_ASSETS_SF) in the Packages view.", "inputSchema": obj(json!({ "package": { "type": "string" } }), &["package"]) },
        { "name": "studio_select", "description": "Select an object of the open package by export index or name; the inspector shows it.", "inputSchema": obj(json!({ "index": { "type": "integer" }, "name": { "type": "string" } }), &[]) },
        { "name": "studio_set_mode", "description": "Show a view: packages, assembly, audio, mod or map.", "inputSchema": obj(json!({ "mode": { "type": "string", "enum": ["packages", "assembly", "audio", "mod", "map"] } }), &["mode"]) },
        { "name": "studio_commands", "description": "Every command of the Studio: id, title, group, keys, whether it can run now.", "inputSchema": obj(json!({}), &[]) },
        { "name": "studio_run_command", "description": "Run a command by id (from studio_commands). Commands that ask a question open a dialog the user answers.", "inputSchema": obj(json!({ "id": { "type": "string" } }), &["id"]) },
        { "name": "studio_open_table", "description": "Open an assembly table (e.g. DATA_SET_ITEMS), optionally at a row index.", "inputSchema": obj(json!({ "table": { "type": "string" }, "row": { "type": "integer" }, "doc": { "type": "string" } }), &["table"]) },
        { "name": "studio_screenshot", "description": "A PNG of the Studio window as it is on screen (bring it to the front first if other windows cover it), scaled down to `maxWidth` pixels (default 1280, 0: full size); `out` also saves it to that file.", "inputSchema": obj(json!({ "maxWidth": { "type": "integer" }, "out": { "type": "string" } }), &[]) },
        { "name": "studio_close_dialog", "description": "Close the open dialog, wizard or palette.", "inputSchema": obj(json!({}), &[]) },
        { "name": "studio_project", "description": "The mod project: status, save, close (saving first).", "inputSchema": obj(json!({ "action": { "type": "string", "enum": ["status", "save", "close"] } }), &["action"]) },
        { "name": "studio_swf", "description": "The Scaleform movie (SwfMovie) editor of the window, the same one the user sees: every call selects the movie, so the window follows. A movie is `index` or `object` (export path or name) of the open package (open it with studio_open_package), else the selected export. Every action first goes to `sprite` (0: the root timeline, else a sprite character id), `character` (any character) or `node` (movie, frames, texts, scripts, images, linkage, tags), then to `frame`; the place is kept between calls. Actions: `movies` (movies of the open package; with `query` also the workspace's), `open` (go somewhere, `depth` selects an object), `status` (movie summary, place with breadcrumb and selection, pending edits, hidden layers), `tree` (character dictionary: id, kind, linkage names, bounds, deps/usedBy; filter `kind`, `query`, page `limit`/`offset`), `timeline` (display list of the open sprite at the frame: depth, character, name, x/y in pixels, scaleX/scaleY as factors, rotation/skew in degrees, alpha in %, bounds, tags, edited, hidden; plus frame labels and frame scripts), `info` (`section`: texts, scripts (every script with its `key`), images with their external file names, linkage, tags; `query`, paging), `script` (script `key`: decompiled ActionScript 2 `source`, `pcode`, and `problems`). Edits are pending until `apply`, like in the editor: `edit` the object at `depth` of the open sprite and frame (`x`, `y` absolute or `dx`, `dy` relative pixels, `scaleX`, `scaleY`, `rotation`, `skew`, or a raw `matrix`; `alpha` 0–100; `name`, empty removes it; a raw colour transform `cxform` {mul, add}, null removes it), `remove` the object at `depth`, `revert` its pending edits, `text` (`kind` text: an edit-text field's initial text by its `tag`; string: script string `index` of script `tag`; image: external image file name of `tag`), `setScript` (script `key` replaced by ActionScript 2 `source` or by `pcode`; compiled first, errors name the line), `layers` (`hide`/`show`/`solo` depths, `showAll`: the editor's render only, never written), `apply` (writes the pending edits into the package as one undo step; then save with studio_run_command file.save, or keep it in the project), `discard`. Characters (pending until `apply`): `character` (what the editor shows of `character`: a shape's colours and line widths with their keys, a font's glyphs, a static text's records, an edit-text field's properties and usable fonts, a sound's format), `shapeStyle` (`character` shape, `styleKey`, `value` `#rrggbbaa` or a width in pixels for `width:` keys), `shapeSvg` (redraw a shape from `svg` or an SVG `file`, fitted into its bounds unless `fit` false), `editText` (`character` edit-text field, `props`: bounds, font, fontClass, height (px), color `#rrggbbaa`, maxLength, align, margins, indent, leading, variable, the flags…), `staticText` (`character` static text, `record`, new `text` and/or `color`), `addGlyphs` (`character` font, `chars` from the TrueType/OpenType `file`). `export` (`file`; `format` svg (default) or png (`scale`) of `character`, of `frame` of `sprite`, or of a root `frame`; ttf for a font, sound, binary; pending edits included). `compile` (check `source` or `pcode` without a movie). Images (written at once, undoable): `addImage` (`file` PNG/DDS → a new Texture2D named `texture`, its DefineExternalImage2, with `shape` (default true) a shape drawing it, and with `place` (default true) placed on the open sprite and frame at `x`, `y` with instance `name`; pending edits are applied with it), `replaceImage` (`file` into the texture of the image `character` or `tag`), `exportImage` (that texture to `file`, .png or .dds).", "inputSchema": obj(serde_json::from_str(SWF_PROPS).expect("the studio_swf schema parses"), &["action"]) },
        { "name": "studio_map", "description": "The map view of the window (a persistent map and the sublevels it streams), the same one the user sees. `list`: packages holding a level (`persistent` ones stream sublevels). `open` {path}: show a map (path or name). `levels`: its levels with actor counts per category (playerstart, botfactory, spawn, mesh, trigger, blocking, volume, light, sound, emitter, gameplay, navigation, info, other), unsaved state, the selection, the camera and the gizmo's handles on screen. `actors` {category?, filter?, level?}: actors (level, export, name, class, category, location, rotation, info such as TeamIndex), at most 500. `select` {level, export}: select an actor (the inspector shows it, the gizmo is on it). `move` {level, export, location?, rotation?}: set an actor's Location (Unreal units, X Y Z) and/or Rotation (Pitch Yaw Roll, 65536 = 360°); for a mesh inside a StaticMeshCollectionActor the component's Translation/Rotation; an unsaved edit of that level package. `show` {categories?, levels?, shading?}: hide/show categories or levels (true = visible), shading lit|unlit|untextured|wireframe. `kismet` {level, sequence?}: the level's Kismet sequences and one sequence's nodes (connectors with their links). `link` / `unlink` {from, to, output+input | variable}: connect or break a Kismet link in the shown sequence. `palette`: the kinds of Kismet nodes the game uses. `addNode` {nodeClass, x?, y?, originator?, object?}: add a node of that class (copied from an example, links emptied) to the shown sequence; an event fires from `originator` (an actor export of the level), an object variable holds `object`. `removeNode` {node}. `actorPalette` {level?, classes?}: the kinds of actors the workspace's levels place (`inMap`: an example in this map), or with `classes` every placeable class. `addActor` {level?, actorClass | export | newClass (+ mesh?), location?, brush?}: add an actor to a level (default: the persistent one): a copy of `export` (an actor of that level, or a mesh of a collection) or of the example of `actorClass`, or a new actor of `newClass` built from its class defaults with `mesh` {path, index} on its mesh component, a volume class shaped by `brush` (default a 256 cube); at `location` (Unreal units), else where the view looks; returns the new export, selected. `removeActor` {level, export}. `heavyWalls` {on?}: actors flagged m_bCanBaseOn False (rooftops; on walls, heavy walls) and colour them in the view; `canBaseOn` {level, export, value}: set the flag. `export` {out, levels?, skip?, volumes?, textureSize?}: the map as .glb or .fbx (by `out`). `matinee` {level, export}: open a SeqAct_Interp's timeline (groups, tracks, keys, driven actors); `setKey` {track, curve, key, time?, value?, mode?, arrive?, leave?, fields?, values?}, `addKey` {track, curve, time, value?, mode?, values?, fields?}, `removeKey` {track, curve, key} edit it (`curve` is a curve's `path` from the timeline; modes CIM_Linear|CIM_CurveAuto|CIM_CurveAutoClamped|CIM_CurveUser|CIM_CurveBreak|CIM_Constant; `fields` {member: text} for timed keys such as EventName `'Name'`; `values` {other curve path: value} for the same key of the track's other curves); `setInterpMode` {track, curve, key, mode} and `setTangents` {track, curve, key, arrive?, leave?} are shorthands; `keyAtPlayhead` {track}: a key at the preview time (a move key from where its actor is drawn). `matinees` {level}: the level's Matinee actions. `newMatinee` {level?, actor?, length?, sequence?}: a new Matinee with a group and relative move track driving `actor` when given. `addGroup` {name, director?}, `removeGroup` {group}, `setGroup` {group, name?, color?}. `addTrack` {group, trackClass, property?}: InterpTrackMove, Event, AkEvent, Sound, AnimControl, Toggle, Visibility, Fade, Director (director group only), FloatProp/VectorProp/ColorProp (`property` e.g. Brightness, DrawScale3D, LightColor), FloatMaterialParam/VectorMaterialParam (`property` = the parameter); `removeTrack` {track}. `setProps` {object, props}: properties of the action (bLooping, PlayRate, bForceStartPos…), the data (InterpLength, EdSectionStart/End), a group or a track (bDisableTrack, MoveFrame, RotMode, LookAtGroupName, TrackTitle). `connect` {group, export, remove?}: make an actor of the timeline's level driven by a group. `matineeTime` {time}: preview the timeline at a time (poses, hidden, tinted actors). `collision` {on?, only?}: draw collision (volumes' convex hulls, meshes' simple collision or triangles, coloured by what they block) over the view, or instead of it with `only`; returns counts by kind:blocks, the volumes and the meshes whose simple collision has no body setup. `brush` {level, export}: a volume's shape. `setBrush` {level, export, brush}: rebuild a volume's shape: {kind: box (x, y, z, hollow, wallThickness) | cylinder (z, outerRadius, innerRadius, sides, alignToSide, hollow) | cone (z, capZ, outerRadius, innerRadius, sides, alignToSide, hollow) | sphere (radius, sphereExtrapolation 1-5) | mesh (mesh {path, index} or {object}, fit hull|bounds) | hull (points) | polys (polys)}. `bake` {scope: edited (default) | selected (level+export, else the selection) | level (levels), shadowSamples?, skySamples?, indirectRays?, albedo?, environmentScale?, compare?}: bake static light maps into new light map textures of the levels, one undo step per level. `lightInfo` {level, export}: a mesh's light map. `navmesh` {navOp?, faces?, vertex?, location?, colour?}: the persistent level's Havok nav mesh: status (default) | show | hide | pick | deleteFaces | cutRegion | moveVertex (`vertex` to `location`, Unreal units). `undo`: take back the last map edit. Save with studio_run_command file.save.", "inputSchema": obj(serde_json::from_str(MAP_PROPS).expect("the studio_map schema parses"), &["action"]) },
        { "name": "studio_mod", "description": "The Mod view: status (files and problems), refresh, build (into the project's builds/ or asks where). Publishing is left to the user.", "inputSchema": obj(json!({ "action": { "type": "string", "enum": ["status", "refresh", "build"] } }), &["action"]) }
    ])
}

/// A random token (no crypto crate: the OS's randomness through std's hasher seeds plus time).
fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for k in 0..4 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(k);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// Start the server; UI calls arrive on the returned receiver.
pub fn start(backend: Backend) -> Result<(Mcp, mpsc::UnboundedReceiver<UiCall>), String> {
    let port: u16 = std::env::var("TEMPEST_STUDIO_MCP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(1423);
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    let token = token();
    let url = format!("http://127.0.0.1:{port}/mcp");
    if let Some(f) = crate::studio::state_file("mcp.json") {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string_pretty(&json!({ "url": url, "token": token })).unwrap_or_default());
    }
    let (tx, rx) = mpsc::unbounded::<UiCall>();
    let t = token.clone();
    std::thread::Builder::new()
        .name("mcp".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let (backend, tx, t) = (backend.clone(), tx.clone(), t.clone());
                std::thread::spawn(move || {
                    let _ = serve(stream, &backend, &tx, &t);
                });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok((Mcp { url, token }, rx))
}

fn respond(mut s: &TcpStream, status: &str, body: &str, session: Option<&str>) -> std::io::Result<()> {
    let extra = session.map(|id| format!("Mcp-Session-Id: {id}\r\n")).unwrap_or_default();
    write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}", body.len())
}

fn serve(stream: TcpStream, backend: &Backend, tx: &mpsc::UnboundedSender<UiCall>, token: &str) -> std::io::Result<()> {
    let mut r = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut len = 0usize;
    let mut auth = String::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => len = v.trim().parse().unwrap_or(0),
                "authorization" => auth = v.trim().to_string(),
                _ => {}
            }
        }
    }
    let mut body = vec![0u8; len.min(64 << 20)];
    r.read_exact(&mut body)?;
    if !path.starts_with("/mcp") {
        return respond(&stream, "404 Not Found", "{}", None);
    }
    if auth != format!("Bearer {token}") {
        return respond(&stream, "401 Unauthorized", r#"{"error":"bad or missing bearer token (see mcp.json)"}"#, None);
    }
    if method != "POST" {
        return respond(&stream, "405 Method Not Allowed", "{}", None);
    }
    let req: J = serde_json::from_slice(&body).unwrap_or(J::Null);
    // Notifications get no answer.
    if req.get("id").is_none() {
        return respond(&stream, "202 Accepted", "", None);
    }
    let id = req["id"].clone();
    let result = match req["method"].as_str().unwrap_or("") {
        "initialize" => Ok(json!({
            "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "tempest-studio-native", "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("").to_string();
            let args = req["params"]["arguments"].clone();
            if name == "studio_screenshot" {
                let reply = match screenshot(tx, &args) {
                    Ok((png, info)) => json!({ "content": [{ "type": "image", "data": base64::engine::general_purpose::STANDARD.encode(&png), "mimeType": "image/png" }, { "type": "text", "text": info.to_string() }] }),
                    Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                };
                let reply = json!({ "jsonrpc": "2.0", "id": id, "result": reply });
                return respond(&stream, "200 OK", &reply.to_string(), None);
            }
            let out = match name.as_str() {
                "upk_call" => upk_call(backend, &args),
                "studio_swf" => swf_call(backend, tx, args),
                "studio_map" => map_call(backend, tx, args),
                _ => ui_call(tx, name, args),
            };
            Ok(match out {
                Ok(v) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }] }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            })
        }
        m => Err(json!({ "code": -32601, "message": format!("unknown method {m}") })),
    };
    let reply = match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    };
    let session = (req["method"] == "initialize").then(|| token.get(..16).unwrap_or("session").to_string());
    respond(&stream, "200 OK", &reply.to_string(), session.as_deref())
}

fn upk_call(backend: &Backend, args: &J) -> Result<J, String> {
    let method = args["method"].as_str().ok_or("missing method")?;
    let params = if args["params"].is_object() { args["params"].clone() } else { json!({}) };
    let r = futures::executor::block_on(backend.call(method, params))?;
    let mut json = r.json;
    // `{"$bin": n}` markers become base64.
    fn walk(v: &mut J, bins: &[Vec<u8>]) {
        match v {
            J::Object(m) => {
                if let Some(n) = m.get("$bin").and_then(|n| n.as_u64()) {
                    if let Some(b) = bins.get(n as usize) {
                        *v = json!({ "base64": base64::engine::general_purpose::STANDARD.encode(b), "bytes": b.len() });
                        return;
                    }
                }
                for x in m.values_mut() {
                    walk(x, bins);
                }
            }
            J::Array(a) => a.iter_mut().for_each(|x| walk(x, bins)),
            _ => {}
        }
    }
    walk(&mut json, &r.bins);
    Ok(json)
}

/// The window once nothing is in flight: its id from the UI thread, the picture taken here.
fn screenshot(tx: &mpsc::UnboundedSender<UiCall>, args: &J) -> Result<(Vec<u8>, J), String> {
    let id = ui_call(tx, "studio_screenshot".into(), J::Null)?["window"].as_i64().ok_or("no window id")?;
    // A frame for what the last call changed.
    std::thread::sleep(std::time::Duration::from_millis(150));
    let full = args["maxWidth"].as_u64() == Some(0);
    let (png, w, h) = crate::screenshot::capture(id, if full { 0 } else { args["maxWidth"].as_u64().unwrap_or(1280) as u32 })?;
    if let Some(out) = args["out"].as_str() {
        std::fs::write(out, &png).map_err(|e| format!("{out}: {e}"))?;
    }
    let info = json!({ "width": w, "height": h, "bytes": png.len(), "out": args["out"] });
    Ok((png, info))
}

/// Until no backend call or build is in flight (at most a minute).
fn wait_idle() {
    let start = std::time::Instant::now();
    while crate::backend::WORK.load(std::sync::atomic::Ordering::SeqCst) > 0 && start.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

fn ui_call(tx: &mpsc::UnboundedSender<UiCall>, tool: String, args: J) -> Result<J, String> {
    // What a tool reads or does starts from a settled state, and an action
    // answers once what it started has finished (a package opened, a build).
    wait_idle();
    let reads = matches!(tool.as_str(), "studio_state" | "studio_commands");
    let out = ui_call_now(tx, tool, args);
    if !reads {
        std::thread::sleep(std::time::Duration::from_millis(60));
        wait_idle();
    }
    out
}

fn ui_call_now(tx: &mpsc::UnboundedSender<UiCall>, tool: String, args: J) -> Result<J, String> {
    let (reply, rx) = oneshot::channel();
    tx.unbounded_send(UiCall { tool, args, reply }).map_err(|_| "the Studio has closed".to_string())?;
    futures::executor::block_on(rx).map_err(|_| "the Studio dropped the call".to_string())?
}

/// `studio_map` in phases: act (retried once the lists it needs are read),
/// then once nothing is in flight read the answer, or call the backend.
fn map_call(backend: &Backend, tx: &mpsc::UnboundedSender<UiCall>, a: J) -> Result<J, String> {
    let mut what = ui_call(tx, "studio_map_act".into(), a.clone())?;
    for _ in 0..4 {
        if what.get("$retry").is_none() {
            break;
        }
        what = ui_call(tx, "studio_map_act".into(), a.clone())?;
    }
    if let Some(b) = what.get("$backend").or_else(|| what.get("$then")) {
        let r = futures::executor::block_on(backend.call(b["method"].as_str().unwrap_or(""), b["params"].clone()))?;
        return Ok(r.json);
    }
    if let Some(w) = what.get("$read") {
        return ui_call_now(tx, "studio_map_read".into(), json!({ "what": w }));
    }
    Ok(what)
}

/// `studio_swf` in phases: select the movie and let it load, go and act,
/// then (once nothing is in flight) read the answer: the editor's state, a
/// backend call, or a PNG rendered by Ruffle (`studio::swf::tool`).
fn swf_call(backend: &Backend, tx: &mpsc::UnboundedSender<UiCall>, a: J) -> Result<J, String> {
    let action = a["action"].as_str().unwrap_or("").to_string();
    let compile = |a: &J| -> Result<J, String> {
        let p = match (a["source"].as_str(), a["pcode"].as_str()) {
            (Some(s), _) => json!({ "source": s }),
            (None, Some(p)) => json!({ "pcode": p }),
            _ => return Err("give source (ActionScript) or pcode".into()),
        };
        futures::executor::block_on(backend.call("swf.compile", p)).map(|r| r.json)
    };
    if action == "compile" {
        return compile(&a);
    }
    let mut a = a;
    if action == "setScript" {
        // A pending edit that does not compile would fail every view.
        let r = compile(&a)?;
        a["$bytes"] = r["bytes"].clone();
    }
    let opened = ui_call(tx, "studio_swf_open".into(), a.clone())?;
    if action == "movies" {
        let Some(q) = a["query"].as_str() else { return Ok(opened) };
        let mut workspace = Vec::new();
        let mut truncated = false;
        for class in ["SwfMovie", "GFxMovieInfo"] {
            let r = futures::executor::block_on(backend.call("workspace.search", json!({ "query": q, "class": class, "limit": a["limit"].as_u64().unwrap_or(100) })))?;
            workspace.extend(r.json["results"].as_array().cloned().unwrap_or_default());
            truncated |= r.json["truncated"].as_bool() == Some(true);
        }
        return Ok(json!({ "openPackage": opened["openPackage"], "workspace": workspace, "truncated": truncated }));
    }
    let what = ui_call(tx, "studio_swf_act".into(), a.clone())?;
    if what.get("$read").is_some() {
        return ui_call_now(tx, "studio_swf_read".into(), json!({ "what": what, "args": a }));
    }
    if let Some(b) = what.get("$backend") {
        let r = futures::executor::block_on(backend.call(b["method"].as_str().unwrap_or(""), b["params"].clone()))?;
        let mut out = r.json;
        if let (Some(o), Some(extra)) = (out.as_object_mut(), what["$extra"].as_object()) {
            for (k, v) in extra {
                o.insert(k.clone(), v.clone());
            }
        }
        // Binary payloads (a texture.import has none worth sending) are left out.
        return Ok(out);
    }
    if let Some(p) = what.get("$png") {
        let out = p["out"].as_str().unwrap_or("").to_string();
        let (png, w, h) = futures::executor::block_on(crate::studio::swf::MovieEditor::render_png(backend.clone(), p["params"].clone(), p["scale"].as_u64().unwrap_or(1) as u32, None))?;
        std::fs::write(&out, &png).map_err(|e| e.to_string())?;
        return Ok(json!({ "out": out, "width": w, "height": h, "bytes": png.len() }));
    }
    Ok(what)
}
