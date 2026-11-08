// MCP server of the Studio: agents drive the running app over Streamable
// HTTP on localhost. Tools call the `upk serve` backend of a window
// (`upk_call`, every method of the IPC contract), read and change what the
// window shows (workspace, package, selection, assembly table, view), run
// the Studio's commands, answer its confirmation dialogs, work with mod
// projects and the mod cooker, edit Scaleform movies (`studio_swf`), evaluate
// code against the renderer stores (`studio_eval`) and take screenshots.
// The endpoint and its bearer token are written to `<userData>/mcp.json`
// and shown in the toolbar's MCP dialog.
import { randomBytes, randomUUID } from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import { isInitializeRequest } from "@modelcontextprotocol/sdk/types.js";
import { app, BrowserWindow } from "electron";
import { z } from "zod";
import { handle } from "./ipc/handle.js";
import { upkFor } from "./upk.js";
import type { McpInfo, UpkFrame, UpkMethod } from "./ipc/contract.js";

const PORT = Number(process.env.TEMPEST_STUDIO_MCP_PORT ?? 1423);
/** Tool results larger than this are cut, with a note. */
const MAX_TEXT = 2_000_000;

let info: McpInfo = { url: "", token: "", configPath: "", running: false };
let createWindowHook: ((options: { dir?: string; files?: string[] }) => BrowserWindow) | null =
	null;

/** Register how a new window is created (main.ts owns that). */
export function onCreateWindow(fn: typeof createWindowHook): void {
	createWindowHook = fn;
}

export function mcpInfo(): McpInfo {
	return info;
}

// ----- windows -----------------------------------------------------------------

function windows(): BrowserWindow[] {
	return BrowserWindow.getAllWindows().filter((w) => !w.isDestroyed());
}

/** The window a tool targets: by WebContents id, else the focused one, else the first. */
function target(id?: number): BrowserWindow {
	const all = windows();
	if (id !== undefined) {
		const w = all.find((w) => w.webContents.id === id);
		if (!w) throw new Error(`no window ${id} (studio_windows lists them)`);
		return w;
	}
	const w = BrowserWindow.getFocusedWindow() ?? all[0];
	if (!w) throw new Error("the Studio has no window open");
	return w;
}

/** Run code in a window against the stores; `code` is the body of an async function
 * with the stores (`studio`, `assembly`, `rpc`, `session`, `ui`, `tabs`, `commands`,
 * `nav`, `cooker`, `map`, `matineeEd`, `swfDoc`, `swfTool`) and `watch` in scope. The returned value comes back as JSON. */
async function evalIn(win: BrowserWindow, code: string): Promise<unknown> {
	const wrapped = `(async () => {
		const api = window.__tempest;
		if (!api) throw new Error("the renderer is not ready");
		const { studio, assembly, rpc, session, ui, tabs, commands, nav, cooker, map, matineeEd, collision, swfDoc, swfTool } = api;
		${WATCH_JS}
		const __result = await (async () => { ${code} })();
		return JSON.stringify(__result === undefined ? null : __result);
	})()`;
	const text = (await win.webContents.executeJavaScript(wrapped, true)) as string;
	return JSON.parse(text);
}

/** `watch(promise, ms)`: waits for an action, but returns early when it asks for a
 * confirmation (the agent answers with studio_confirm) or takes longer than `ms`. */
const WATCH_JS = `const asking = () => ui.asking ? { title: ui.asking.title, message: ui.asking.message, items: ui.asking.items, confirm: ui.asking.confirm, cancel: ui.asking.cancel ?? "Cancel", alt: ui.asking.alt } : null;
	const watch = async (promise, ms = 120000) => {
		let stop = false;
		const outcome = await Promise.race([
			Promise.resolve(promise).then((value) => ({ status: "done", value }), (error) => ({ status: "error", error: String(error?.message ?? error) })),
			(async () => { while (!stop && !ui.asking) await new Promise((r) => setTimeout(r, 50)); return { status: "confirmation" }; })(),
			new Promise((r) => setTimeout(() => r({ status: "running" }), ms)),
		]);
		stop = true;
		if (outcome.status === "confirmation") outcome.confirmation = asking();
		return outcome;
	};`;

const STATE_JS = `return {
	window: undefined,
	mode: studio.mode,
	workspace: studio.workspace ? { dir: studio.workspace.dir, packages: studio.workspace.packages.length, scriptPackages: studio.workspace.scriptPackages } : null,
	package: studio.pkg ? { name: studio.pkg.name, path: studio.pkg.path, exports: studio.pkg.exports.length, imports: studio.pkg.imports.length, dirty: studio.isDirty } : null,
	selected: studio.selectedRow ? { index: studio.selectedRow.i, path: studio.selectedRow.path, class: studio.selectedRow.class } : null,
	dirtyPackages: studio.dirty,
	undo: studio.undoLabels,
	busy: studio.busy,
	backendError: studio.backendError,
	project: session.project ? { name: session.project.name, dir: session.project.dir, file: session.file?.path, state: session.projectState } : null,
	workspaceFile: session.file ? { path: session.file.path, mode: session.file.mode } : null,
	editedFiles: [...studio.dirty, ...assembly.dirtyDocs.map((d) => d.path)],
	dialog: ui.dialog,
	confirmation: ui.asking ? { title: ui.asking.title, message: ui.asking.message, items: ui.asking.items, confirm: ui.asking.confirm, alt: ui.asking.alt } : null,
	tabs: tabs.list.map((t) => ({ id: t.id, active: t.id === tabs.active, ...tabs.label(t.place) })),
	history: { back: nav.label(-1), forward: nav.label(1) },
	cooker: { busy: cooker.busy, files: cooker.shown.length, problems: cooker.problems, built: cooker.cooked },
	assembly: assembly.summary ? {
		docs: assembly.docs.map((d) => ({ id: d.id, path: d.path, dirty: d.dirty, tables: d.tables.length })),
		table: assembly.page ? { doc: assembly.page.doc, table: assembly.page.table, path: assembly.page.path, total: assembly.page.total, offset: assembly.page.offset, filter: assembly.filter, sort: assembly.sort, desc: assembly.desc } : null,
		selected: assembly.selected,
	} : null,
};`;

async function stateOf(win: BrowserWindow): Promise<Record<string, unknown>> {
	const s = (await evalIn(win, STATE_JS)) as Record<string, unknown>;
	s.window = win.webContents.id;
	s.title = win.getTitle();
	return s;
}

// ----- backend results -----------------------------------------------------------

/** A framed backend reply as JSON text: payloads become base64 strings. */
function frameToJson(frame: UpkFrame): unknown {
	const value = frame.json ? JSON.parse(Buffer.from(frame.json).toString("utf8")) : frame.value;
	if (frame.bins.length === 0) return value;
	const place = (v: unknown): unknown => {
		if (!v || typeof v !== "object") return v;
		if (Array.isArray(v)) return v.map(place);
		const r = v as Record<string, unknown>;
		if (typeof r.$bin === "number" && Object.keys(r).length === 1) {
			const b = frame.bins[r.$bin];
			return b ? Buffer.from(b).toString("base64") : "";
		}
		for (const k of Object.keys(r)) r[k] = place(r[k]);
		return r;
	};
	return place(value);
}

function text(value: unknown): { content: { type: "text"; text: string }[] } {
	let s = typeof value === "string" ? value : JSON.stringify(value, null, 1);
	if (s.length > MAX_TEXT) {
		s = `${s.slice(0, MAX_TEXT)}\n… (cut at ${MAX_TEXT} characters; page the request)`;
	}
	return { content: [{ type: "text", text: s }] };
}

function failure(error: unknown) {
	return {
		isError: true,
		content: [
			{ type: "text" as const, text: error instanceof Error ? error.message : String(error) },
		],
	};
}

async function guarded<T>(fn: () => Promise<T>): Promise<T | ReturnType<typeof failure>> {
	try {
		return await fn();
	} catch (error) {
		return failure(error);
	}
}

// ----- the server ----------------------------------------------------------------

function buildServer(): McpServer {
	const server = new McpServer({ name: "tempest-studio", version: app.getVersion() });
	const win = z
		.number()
		.int()
		.optional()
		.describe(
			"WebContents id of the window (from studio_windows); default: the focused window",
		);

	server.registerTool(
		"studio_windows",
		{
			title: "List windows",
			description:
				"Every Studio window with its workspace, open package, selection and mode. Each window has its own upk backend.",
			inputSchema: {},
		},
		() => guarded(async () => text(await Promise.all(windows().map(stateOf)))),
	);

	server.registerTool(
		"studio_state",
		{
			title: "Window state",
			description:
				"What a window shows: view (packages, assembly, mod), workspace, package, selected export, edited files, undo labels, assembly table, mod project and its save state, open dialog, a pending confirmation, tabs, history and the mod cooker's summary.",
			inputSchema: { window: win },
		},
		({ window }) => guarded(async () => text(await stateOf(target(window)))),
	);

	server.registerTool(
		"upk_call",
		{
			title: "Call the backend",
			description:
				"Call any method of the window's `upk serve` backend (the UpkMethods of electron/src/ipc/contract.ts): workspace.search, package.open, object.get, prop.set, object.list, object.duplicate, texture.export, mesh.export, mesh.materials, mesh.setMaterial, mesh.ownMaterial, mesh.ownMaterials, material.newInstance, material.setTexture, package.create, object.add, object.addGroup, anim.info, anim.export, anim.import, anim.edit, material.preview, particle.system, swf.info, swf.tree, swf.timeline, swf.script, mcts.rows, mcts.set, mcts.addRow, mcts.usages, mod.changes, mod.preview, delta.create, twrk.read, … Binary payloads come back as base64. Edits made here are not reflected in the window until it reloads (use studio_open_package / studio_select to refresh); prefer studio_eval with the stores (studio.setProp…) when the window should follow, so the edit also reaches undo, the project and the cooker.",
			inputSchema: {
				method: z.string().describe("method name, e.g. workspace.search"),
				params: z.record(z.string(), z.unknown()).optional().describe("parameters object"),
				window: win,
			},
		},
		({ method, params, window }) =>
			guarded(async () => {
				const frame = (await upkFor(target(window).webContents).call(
					method as UpkMethod,
					params ?? {},
				)) as UpkFrame;
				return text(frameToJson(frame));
			}),
	);

	server.registerTool(
		"studio_open_workspace",
		{
			title: "Open workspace",
			description: "Open a cooked game folder (CookedPCConsole) in a window.",
			inputSchema: { dir: z.string(), window: win },
		},
		({ dir, window }) =>
			guarded(async () => {
				const w = target(window);
				const ok = await evalIn(
					w,
					`return await studio.openWorkspace(${JSON.stringify(dir)});`,
				);
				return text({ ok, state: await stateOf(w) });
			}),
	);

	server.registerTool(
		"studio_open_package",
		{
			title: "Open package",
			description:
				"Open a .upk in the window (and optionally select an export index). Packages outside the workspace folder are opened as external.",
			inputSchema: { path: z.string(), select: z.number().int().optional(), window: win },
		},
		({ path: p, select, window }) =>
			guarded(async () => {
				const w = target(window);
				await evalIn(
					w,
					`studio.mode = "packages"; await studio.openFiles([${JSON.stringify(p)}]); ${select === undefined ? "" : `await studio.select(${select});`}`,
				);
				return text(await stateOf(w));
			}),
	);

	server.registerTool(
		"studio_select",
		{
			title: "Select export",
			description: "Select an export of the open package (the inspector shows it).",
			inputSchema: { index: z.number().int(), window: win },
		},
		({ index, window }) =>
			guarded(async () => {
				const w = target(window);
				await evalIn(w, `await studio.select(${index});`);
				return text(await stateOf(w));
			}),
	);

	server.registerTool(
		"studio_set_mode",
		{
			title: "Switch mode",
			description:
				"Show the Packages view, the Assembly view, the map view (map) or the mod cooker (mod).",
			inputSchema: { mode: z.enum(["packages", "assembly", "map", "mod"]), window: win },
		},
		({ mode, window }) =>
			guarded(async () => {
				const w = target(window);
				await evalIn(w, `studio.mode = ${JSON.stringify(mode)};`);
				return text(await stateOf(w));
			}),
	);

	server.registerTool(
		"studio_open_table",
		{
			title: "Open assembly table",
			description:
				"In the Assembly editor, show a table of a document (assembly, INT, FRA…), optionally a nested row set under a row path, and select a row.",
			inputSchema: {
				doc: z.string().default("assembly"),
				table: z.string().describe("e.g. DATA_SET_ITEMS"),
				path: z
					.array(z.tuple([z.string(), z.number().int()]))
					.optional()
					.describe('row path for a nested row set, e.g. [["DATA_SET_ITEMS", 12]]'),
				select: z.number().int().optional(),
				filter: z.string().optional(),
				window: win,
			},
		},
		({ doc, table, path: rowPath, select, filter, window }) =>
			guarded(async () => {
				const w = target(window);
				await evalIn(
					w,
					`studio.mode = "assembly";
					for (let i = 0; i < 200 && !assembly.summary; i++) await new Promise((r) => setTimeout(r, 100));
					await assembly.openTable(${JSON.stringify(doc)}, ${JSON.stringify(table)}, ${JSON.stringify(rowPath ?? [])}, ${select ?? null});
					${filter ? `await assembly.setFilter(${JSON.stringify(filter)});` : ""}`,
				);
				return text(await stateOf(w));
			}),
	);

	server.registerTool(
		"studio_eval",
		{
			title: "Evaluate in the renderer",
			description:
				"Run JavaScript in a window with the renderer stores in scope: `studio` (packages, state.svelte.ts), `assembly` (assembly.svelte.ts), `rpc` (typed backend calls), `session` (projects and workspace files, session.svelte.ts), `cooker` (mod cooker, cooker.svelte.ts), `map` (map view and Kismet, map.svelte.ts; studio_map covers it), `collision` (collision view and volume shapes, collision.svelte.ts), `commands` (command registry), `ui` (dialogs, ui.asking = pending confirmation), `tabs`, `nav`, `swfDoc(path, index)` (the movie editor's document of a SwfMovie export, swf.svelte.ts; studio_swf covers it); plus `watch(promise, ms)` which returns early with the question when the action asks for a confirmation. The code is the body of an async function and its return value comes back as JSON. Example: `return (await rpc('workspace.search', { query: 'zhin', limit: 5 })).results;`",
			inputSchema: { code: z.string(), window: win },
		},
		({ code, window }) => guarded(async () => text(await evalIn(target(window), code))),
	);

	server.registerTool(
		"studio_swf",
		{
			title: "Movie editor",
			description: [
				"The Scaleform movie (SwfMovie / GFxMovieInfo export) editor of a window, the same one the user sees: every call selects the movie, so the window follows. A movie is `path` (package, default: the open one) plus `index` or `object` (export path or name), else the selected export.",
				"Every action first goes to `sprite` (0: the root timeline, else a sprite character id), `character` (any character) or `node` (movie, frames, texts, scripts, images, linkage, tags), then to `frame`; the place is kept between calls.",
				"Actions: `movies` (movies of the open package; with `query` also the workspace's), `open` (go somewhere, `depth` selects an object), `status` (movie summary, place with breadcrumb and selection, pending edits, hidden layers), `tree` (character dictionary: id, kind, linkage names, bounds, deps/usedBy; filter `kind`, `query`, page `limit`/`offset`), `timeline` (display list of the open sprite at the frame: depth, character, name, x/y in pixels, scaleX/scaleY as factors, rotation/skew in degrees, alpha in %, bounds, tags, edited, hidden; plus frame labels and frame scripts), `info` (`section`: texts, scripts (every script with its `key`: classes, frame scripts of the root and of sprites, clip events, buttons), images with their external file names, linkage, tags; `query`, paging), `script` (script `key`: decompiled ActionScript 2 `source`, `pcode`, and `problems`, parts kept as P-code).",
				"Edits are pending until `apply`, like in the editor: `edit` the object at `depth` of the open sprite and frame (`x`, `y` absolute or `dx`, `dy` relative pixels, `scaleX`, `scaleY`, `rotation`, `skew`, or a raw `matrix`; `alpha` 0–100; `name`, empty removes it), `remove` the object at `depth`, `revert` its pending edits, `text` (`kind` text: an edit-text field's initial text by its `tag`; string: script string `index` of script `tag`; image: external image file name of `tag`), `setScript` (script `key` replaced by ActionScript 2 `source` (as `script` returns it: class syntax, `__pcode { … }` blocks, `__cast`/`__extends` built-ins) or by `pcode`; compiled first, errors name the line), `layers` (`hide`/`show`/`solo` depths, `showAll`: the editor's render only, never written to the movie), `apply` (writes the pending edits into the package as one undo step; then save with studio_run_command file.save, or keep it in the project/mod cooker), `discard` (drops the pending edits).",
				"Characters (pending until `apply` like the rest): `character` (what the editor shows of `character`: a shape's fill/line/gradient-stop colours and line widths with their keys, a font's glyphs, a static text's records, an edit-text field's properties and usable fonts, a sound's format), `shapeStyle` (`character` shape, `styleKey` from `character`, `value` `#rrggbbaa` or a width in pixels for `width:` keys), `shapeSvg` (redraw a shape from `svg` or an SVG `file`, fitted into its bounds unless `fit` false), `editText` (`character` edit-text field, `props`: bounds, font, fontClass, height (px), color `#rrggbbaa`, maxLength, align, margins, indent, leading, variable, wordWrap, multiline, password, readOnly, autoSize, noSelect, border, html, useOutlines…), `staticText` (`character` static text, `record`, new `text` and/or `color`), `addGlyphs` (`character` font, `chars` taken from the TrueType/OpenType `file`). `edit` also takes a raw colour transform `cxform` ({mul, add}, RGBA; null removes it). `export` (`file`; `format` svg (default) or png (`scale`) of `character`, of `frame` of `sprite`, or of a root `frame`; ttf for a font, sound, binary for those characters; pending edits included). `compile` (check `source` or `pcode` without a movie: errors name the line, else the P-code).",
				"Images (written at once, undoable, not pending): `addImage` (`file` PNG/DDS → a new Texture2D named `texture` next to the movie's images, its DefineExternalImage2, with `shape` (default true) a shape drawing it, and with `place` (default true) that shape on the open sprite and frame at `x`, `y` (top-left, pixels) with instance `name`; pending edits are applied with it), `replaceImage` (`file` PNG/DDS into the texture of the image `character` or `tag`; the movie keeps drawing it at its listed size), `exportImage` (that texture to `file`, .png or .dds).",
				"See the result with studio_screenshot.",
			].join(" "),
			inputSchema: {
				action: z.enum([
					"movies",
					"open",
					"status",
					"tree",
					"timeline",
					"info",
					"script",
					"setScript",
					"edit",
					"remove",
					"revert",
					"text",
					"layers",
					"apply",
					"discard",
					"addImage",
					"replaceImage",
					"exportImage",
					"character",
					"shapeStyle",
					"shapeSvg",
					"editText",
					"staticText",
					"addGlyphs",
					"export",
					"compile",
				]),
				path: z
					.string()
					.optional()
					.describe("package of the movie (default: the open one)"),
				index: z
					.number()
					.int()
					.optional()
					.describe(
						"export index of the movie; with kind string: the script string's index",
					),
				object: z.string().optional().describe("export path or name of the movie"),
				sprite: z
					.number()
					.int()
					.optional()
					.describe("timeline to open: 0 the root, else a sprite id"),
				character: z.number().int().optional().describe("character to open"),
				node: z
					.enum(["movie", "frames", "texts", "scripts", "images", "linkage", "tags"])
					.optional(),
				frame: z.number().int().optional(),
				depth: z.number().int().optional().describe("object of the open timeline"),
				query: z.string().optional(),
				kind: z
					.string()
					.optional()
					.describe("tree: character kind; text: text, string or image"),
				section: z.enum(["texts", "scripts", "images", "linkage", "tags"]).optional(),
				tag: z
					.number()
					.int()
					.optional()
					.describe("top-level tag index (texts, scripts, images)"),
				text: z.string().optional(),
				key: z
					.string()
					.optional()
					.describe("script / setScript: the script's key from info scripts"),
				source: z.string().optional().describe("setScript: ActionScript 2"),
				pcode: z.string().optional().describe("setScript: P-code"),
				limit: z.number().int().optional(),
				offset: z.number().int().optional(),
				x: z.number().optional(),
				y: z.number().optional(),
				dx: z.number().optional(),
				dy: z.number().optional(),
				scaleX: z.number().optional().describe("factor, 1 = 100%"),
				scaleY: z.number().optional().describe("factor, 1 = 100%"),
				rotation: z.number().optional().describe("degrees"),
				skew: z.number().optional().describe("degrees"),
				matrix: z
					.object({
						a: z.number(),
						b: z.number(),
						c: z.number(),
						d: z.number(),
						tx: z.number(),
						ty: z.number(),
					})
					.partial()
					.optional(),
				alpha: z.number().optional().describe("percent, 0 hides the object"),
				name: z.string().optional().describe("instance name; empty removes it"),
				file: z
					.string()
					.optional()
					.describe(
						"addImage/replaceImage: PNG or DDS; exportImage: output .png/.dds; export: output; shapeSvg: SVG; addGlyphs: TTF/OTF",
					),
				texture: z.string().optional().describe("addImage: name of the new Texture2D"),
				shape: z.boolean().optional().describe("addImage: make a shape drawing it"),
				place: z.boolean().optional().describe("addImage: place the shape on this frame"),
				cxform: z
					.object({
						mul: z.array(z.number()).length(4),
						add: z.array(z.number()).length(4),
					})
					.nullable()
					.optional()
					.describe(
						"edit: colour transform, RGBA multipliers (1 = unchanged) and additions (0–255)",
					),
				styleKey: z
					.string()
					.optional()
					.describe("shapeStyle: fill:g:i, stop:g:i:k, line:g:i, width:g:i"),
				value: z
					.string()
					.optional()
					.describe("shapeStyle: #rrggbbaa, or pixels for width keys"),
				svg: z.string().optional().describe("shapeSvg: the SVG document"),
				fit: z
					.boolean()
					.optional()
					.describe("shapeSvg: fit into the shape's bounds (default true)"),
				props: z
					.record(z.string(), z.unknown())
					.optional()
					.describe("editText: properties to change"),
				record: z.number().int().optional().describe("staticText: the text record"),
				color: z.string().optional().describe("staticText: #rrggbbaa"),
				chars: z.string().optional().describe("addGlyphs: the characters to add"),
				format: z
					.enum(["svg", "png", "ttf", "sound", "binary"])
					.optional()
					.describe("export"),
				scale: z.number().optional().describe("export png: scale (default 1)"),
				hide: z.array(z.number().int()).optional(),
				show: z.array(z.number().int()).optional(),
				solo: z.array(z.number().int()).optional(),
				showAll: z.boolean().optional(),
				window: win,
			},
		},
		({ window, ...args }) =>
			guarded(async () =>
				text(
					await evalIn(target(window), `return await swfTool(${JSON.stringify(args)});`),
				),
			),
	);

	server.registerTool(
		"studio_commands",
		{
			title: "List commands",
			description:
				"Every command of the Studio (what the menus, the palette and the shortcuts run): id, title, what it does, shortcut, and whether it can run now. Run one with studio_run_command.",
			inputSchema: {
				query: z.string().optional().describe("substring of the id, title or group"),
				window: win,
			},
		},
		({ query, window }) =>
			guarded(async () =>
				text(
					await evalIn(
						target(window),
						`const q = ${JSON.stringify((query ?? "").toLowerCase())};
						return commands.list
							.filter((c) => !c.hidden && (!q || (c.id + " " + c.title + " " + c.group).toLowerCase().includes(q)))
							.map((c) => ({ id: c.id, title: c.title, detail: c.detail, group: c.group, keys: c.keys, enabled: commands.enabled(c) }));`,
					),
				),
			),
	);

	server.registerTool(
		"studio_run_command",
		{
			title: "Run a command",
			description:
				'Run a Studio command by id (studio_commands lists them), like a click in a menu: save, discard, undo, open a dialog, switch views, close a tab… Returns when it finishes, or early with `status: "confirmation"` and the question when it asks before doing something destructive: answer with studio_confirm. Commands that open a file picker wait for the user.',
			inputSchema: {
				id: z.string().describe("e.g. file.save, edit.discard, edit.undo, view.mod"),
				timeout: z.number().int().optional().describe("ms to wait (default 120000)"),
				window: win,
			},
		},
		({ id, timeout, window }) =>
			guarded(async () => {
				const w = target(window);
				const outcome = await evalIn(
					w,
					`const c = commands.get(${JSON.stringify(id)});
					if (!c) throw new Error("no command " + ${JSON.stringify(id)} + " (studio_commands lists them)");
					if (!commands.enabled(c)) throw new Error("the command cannot run now: " + c.title);
					return await watch(c.run(), ${timeout ?? 120_000});`,
				);
				return text({ ...(outcome as object), state: await stateOf(w) });
			}),
	);

	server.registerTool(
		"studio_confirm",
		{
			title: "Answer a confirmation",
			description:
				"Answer the confirmation dialog the window shows (studio_state.confirmation): `confirm` (its main button, e.g. Discard), `cancel`, or `alt` (the third button when there is one). Waits for the action to finish.",
			inputSchema: {
				answer: z.enum(["confirm", "cancel", "alt"]),
				window: win,
			},
		},
		({ answer, window }) =>
			guarded(async () => {
				const w = target(window);
				await evalIn(
					w,
					`if (!ui.asking) throw new Error("no confirmation is pending");
					const a = ui.asking;
					if (${JSON.stringify(answer)} === "alt" && !a.alt) throw new Error("this question has no third choice");
					a.answer(${answer === "confirm" ? "true" : answer === "alt" ? '"alt"' : "false"});
					await new Promise((r) => setTimeout(r, 300));
					for (let i = 0; i < 1200 && (studio.busy || cooker.busy || session.projectState === "saving"); i++) await new Promise((r) => setTimeout(r, 100));`,
				);
				return text(await stateOf(w));
			}),
	);

	server.registerTool(
		"studio_close_dialog",
		{
			title: "Close the dialog",
			description: "Close the dialog or command palette the window shows.",
			inputSchema: { window: win },
		},
		({ window }) =>
			guarded(async () => {
				const w = target(window);
				await evalIn(w, `ui.close(); ui.palette = null;`);
				return text(await stateOf(w));
			}),
	);

	server.registerTool(
		"studio_project",
		{
			title: "Mod project",
			description:
				'Work with mod projects (a folder with <id>.twrk, mod-files/ and builds/; edits are kept in the project file, which saves itself). `status`: the current project. `create`: a new project in `dir` for the game folder `gameDir` (a CookedPCConsole folder) with `name`, optional `id` (made from the name), `author`, `version` (semver, default 1.0.0). `open`: a .twrk at `path`. `save`: save now. `close`: back to plain editing; the project\'s edits are unloaded from the window (they stay in the project file). Closing, and opening or creating while a project is open, asks first (Save and close / Discard and close when a save is pending, else Close): the result is then `status: "confirmation"`, answer with studio_confirm. `value` is false when the action was cancelled or failed.',
			inputSchema: {
				action: z.enum(["status", "create", "open", "save", "close"]),
				dir: z.string().optional(),
				gameDir: z.string().optional(),
				name: z.string().optional(),
				id: z.string().optional(),
				author: z.string().optional(),
				version: z.string().optional(),
				path: z.string().optional(),
				window: win,
			},
		},
		(a) =>
			guarded(async () => {
				const w = target(a.window);
				const args = JSON.stringify(a);
				const outcome = await evalIn(
					w,
					`const a = ${args};
					const need = (k) => { if (!a[k]) throw new Error(a.action + " needs " + k); return a[k]; };
					if (a.action === "create") {
						const name = need("name");
						return await watch(session.createProject({
							dir: need("dir"),
							gameDir: need("gameDir"),
							name,
							id: a.id || name.toLowerCase().normalize("NFKD").replace(/[\\u0300-\\u036f]/g, "").replace(/[^a-z0-9._-]+/g, "-").replace(/^[-._]+|[-._]+$/g, "").slice(0, 64),
							author: a.author ?? "",
							version: a.version ?? "1.0.0",
						}), 600000);
					}
					if (a.action === "open") return await watch(session.open(need("path")), 600000);
					if (a.action === "save") {
						if (!session.project) throw new Error("no project is open");
						await session.saveProject();
						return { status: session.projectState };
					}
					if (a.action === "close") {
						if (!session.project) throw new Error("no project is open");
						return await watch(session.closeProject());
					}
					return { status: "done" };`,
				);
				return text({ ...(outcome as object), state: await stateOf(w) });
			}),
	);

	const choice = z.object({
		target: z.string().describe("the game file (entries[].target)"),
		on: z.boolean().optional().describe("false leaves the file out of the mod"),
		mode: z.enum(["delta", "whole"]).optional(),
		source: z.string().optional().describe("entries[].sources[].id to take the changes from"),
	});
	server.registerTool(
		"studio_map",
		{
			title: "Map view",
			description:
				"The map view of a window (a persistent map and the sublevels it streams). `list`: packages holding a level (`persistent` ones stream sublevels). `open` {path}: show a map. `levels`: its levels with actor counts per category (playerstart, botfactory, spawn, mesh, trigger, blocking, volume, light, sound, emitter, gameplay, navigation, info, other) and unsaved state. `actors` {category?, filter?, level?}: actors (level, export, name, class, category, location, rotation, info such as TeamIndex), at most 500. `select` {level, export}: select an actor (the inspector shows it, the gizmo is on it). `move` {level, export, location?, rotation?}: set an actor's Location (Unreal units, X Y Z) and/or Rotation (Pitch Yaw Roll, 65536 = 360°) — for a mesh inside a StaticMeshCollectionActor (`edit: component`) the component's Translation/Rotation; an unsaved edit of that level package. `show` {categories?, levels?, shading?}: hide/show categories or levels (true = visible), shading lit|unlit|untextured|wireframe. `kismet` {level, sequence?}: the level's Kismet sequences and one sequence's nodes (connectors with their links). `link` / `unlink` {from, to, output+input | variable}: connect or break a Kismet link in the shown sequence. `palette`: the kinds of Kismet nodes the game uses. `addNode` {nodeClass, x?, y?, originator?, object?}: add a node of that class (copied from an example, links emptied) to the shown sequence; an event fires from `originator` (an actor export of the level), an object variable holds `object`. `removeNode` {node}. `actorPalette` {level?, classes?}: the kinds of actors the workspace's levels place (`inMap`: an example in this map). `addActor` {level?, actorClass | export | newClass (+ mesh?), location?}: add an actor to a level (default: the persistent one): a copy of `export` (an actor of that level, or a mesh of a collection: duplicated in place) or of the example of `actorClass` (from another map its meshes, materials and textures come along), or a new actor of `newClass` (any placeable class, `actorPalette` with `classes: true` lists them, volumes marked `volume`) built from its class defaults and laid out like cooked actors (no archetype), with `mesh` {path, index} (a StaticMesh/SkeletalMesh of any package) on its mesh component, and for a volume class `brush` (its shape, see setBrush; default a 256 cube); at `location` (Unreal units), else where the view looks; returns the new export, selected. `removeActor` {level, export}: delete an actor (or a collection's mesh) from its level, references to it cleared. `heavyWalls` {on?}: actors flagged m_bCanBaseOn False (rooftops; on walls, heavy walls) and colour them in the view; `canBaseOn` {level, export, value}: set the flag. `export` {out, levels?, skip?, volumes?, textureSize?}: the map as .glb or .fbx (by `out`). `matinee` {level, export}: open a SeqAct_Interp's timeline (groups, tracks, keys, driven actors); `setKey` {track, curve, key, time?, value?}, `addKey` {track, curve, time}, `removeKey` {track, curve, key} edit it (`curve` is a curve's `path` from the timeline; `setKey` also takes `mode` CIM_Linear|CIM_CurveAuto|CIM_CurveAutoClamped|CIM_CurveUser|CIM_CurveBreak|CIM_Constant, `arrive`/`leave` tangents (an auto key becomes CIM_CurveUser), `fields` {member: text} for timed keys such as EventName `'Name'`, `values` {other curve path: value} for the same key of the track's other curves; `addKey` takes `value`, `mode`, `values`, `fields`; tangents of auto keys are recomputed like the engine's AutoSetTangents, a move key's position/rotation/lookup lists change together); `setInterpMode` {track, curve, key, mode} and `setTangents` {track, curve, key, arrive?, leave?} are shorthands; `keyAtPlayhead` {track}: a key at the preview time (a move key from where its actor is drawn). `matinees` {level}: the level's Matinee actions. `newMatinee` {level?, actor?, length?, sequence?}: a new Matinee (SeqAct_Interp + InterpData) in the level's top sequence, with a group and relative move track driving `actor` when given. `addGroup` {name, director?}, `removeGroup` {group}, `setGroup` {group, name?, color?}: groups (a rename keeps the connector and its links). `addTrack` {group, trackClass, property?}: InterpTrackMove, Event, AkEvent, Sound, AnimControl, Toggle, Visibility, Fade, Director (director group only), FloatProp/VectorProp/ColorProp (`property` e.g. Brightness, DrawScale3D, LightColor), FloatMaterialParam/VectorMaterialParam (`property` = the parameter); `removeTrack` {track}. `setProps` {object, props: {Prop: text|number|bool}}: properties of the action (bLooping, PlayRate, bRewindOnPlay, bForceStartPos, ForceStartPosition, bClientSideOnly…), the data (InterpLength, EdSectionStart/End), a group or a track (bDisableTrack, MoveFrame imf_world|imf_relativetoinitial, RotMode imr_keyframed|imr_lookatgroup|imr_ignore, LookAtGroupName, TrackTitle). `connect` {group, export, remove?}: make an actor of the timeline's level driven by a group (a SeqVar_Object linked to the group's connector). `matineeTime` {time}: preview the timeline at a time (poses, hidden, tinted actors). `collision` {on?, only?}: draw collision (volumes' convex hulls, meshes' simple collision or triangles, coloured by what they block) over the view, or instead of it with `only`; returns counts by kind:blocks, the volumes (level, export, class, blocks, hull count) and the meshes whose simple collision has no body setup (they collide with nothing). `brush` {level, export}: a volume's shape (brush polys in actor space, Unreal units; its convex hulls). `setBrush` {level, export, brush}: rebuild a volume's shape: `brush` is {kind: box (x, y, z, hollow, wallThickness) | cylinder (z, outerRadius, innerRadius, sides, alignToSide, hollow) | cone (z, capZ, outerRadius, innerRadius, sides, alignToSide, hollow) | sphere (radius, sphereExtrapolation 1-5) | mesh (mesh {path, index} or {object}, fit hull|bounds) | hull (points) | polys (polys: vertex lists, outward)}; the brush model, BSP and BrushAggGeom are rebuilt like UnrealEd does; an unsaved edit. `bake` {scope: edited (default: meshes added or moved this session) | selected (level+export, else the selection) | level (levels), shadowSamples?, skySamples?, indirectRays?, albedo? (0.08), environmentScale? (1), compare?}: bake static light maps (direct light of the map's static lights, shadows against every level; indirectRays > 0 adds a one-bounce approximation, needed for shade: without it shaded areas come out at about 0.6 of the shipped lighting) into new light map textures of the levels, every LOD of a mesh its own light map, one undo step per level; light and shadow maps nothing uses any more are deleted (`removed`); `compare` reports each mesh's (and LOD's) mean irradiance against its previous light map. `lightInfo` {level, export}: a mesh's light map (lights, textures, scales, decoded mean irradiance). `navmesh` {navOp?, faces?, vertex?, location?, colour?}: the persistent level's Havok nav mesh (faces, edges, regions, clusters, jump links): `navOp` status (default) | show | hide | pick (pick `faces` in the view) | deleteFaces (`faces`) | cutRegion (every face connected to `faces`) | moveVertex (`vertex` to `location`, Unreal units); edits are unsaved, undoable edits of the level (the face AABB tree rebuilt). `undo`: take back the last map edit. Save with studio_run_command file.save.",
			inputSchema: {
				action: z.enum([
					"list",
					"open",
					"levels",
					"actors",
					"select",
					"move",
					"show",
					"kismet",
					"link",
					"unlink",
					"palette",
					"addNode",
					"removeNode",
					"actorPalette",
					"addActor",
					"removeActor",
					"heavyWalls",
					"canBaseOn",
					"export",
					"matinee",
					"setKey",
					"addKey",
					"removeKey",
					"matineeTime",
					"setInterpMode",
					"setTangents",
					"keyAtPlayhead",
					"matinees",
					"newMatinee",
					"addGroup",
					"removeGroup",
					"setGroup",
					"addTrack",
					"removeTrack",
					"setProps",
					"connect",
					"navmesh",
					"undo",
					"collision",
					"brush",
					"setBrush",
					"bake",
					"lightInfo",
				]),
				mode: z.string().optional(),
				arrive: z.union([z.number(), z.array(z.number())]).optional(),
				leave: z.union([z.number(), z.array(z.number())]).optional(),
				fields: z
					.record(z.string(), z.union([z.string(), z.number(), z.boolean()]))
					.optional(),
				values: z.record(z.string(), z.union([z.number(), z.array(z.number())])).optional(),
				group: z.number().int().optional(),
				name: z.string().optional(),
				color: z.array(z.number()).optional(),
				director: z.boolean().optional(),
				trackClass: z.string().optional(),
				property: z.string().optional(),
				props: z
					.record(z.string(), z.union([z.string(), z.number(), z.boolean()]))
					.optional(),
				actor: z.number().int().optional(),
				remove: z.boolean().optional(),
				length: z.number().optional(),
				brush: z.record(z.string(), z.unknown()).optional(),
				only: z.boolean().optional(),
				scope: z.enum(["selected", "edited", "level"]).optional(),
				shadowSamples: z.number().int().optional(),
				skySamples: z.number().int().optional(),
				indirectRays: z.number().int().optional(),
				albedo: z.number().optional(),
				environmentScale: z.number().optional(),
				compare: z.boolean().optional(),
				navOp: z
					.enum([
						"status",
						"show",
						"hide",
						"pick",
						"deleteFaces",
						"cutRegion",
						"moveVertex",
					])
					.optional(),
				faces: z.array(z.number().int()).optional(),
				vertex: z.number().int().optional(),
				colour: z.enum(["region", "cluster"]).optional(),
				nodeClass: z.string().optional(),
				actorClass: z.string().optional(),
				newClass: z.string().optional(),
				mesh: z.object({ path: z.string(), index: z.number().int() }).optional(),
				classes: z.boolean().optional(),
				x: z.number().optional(),
				y: z.number().optional(),
				originator: z.number().int().optional(),
				object: z.number().int().optional(),
				node: z.number().int().optional(),
				on: z.boolean().optional(),
				value: z.union([z.boolean(), z.number(), z.array(z.number())]).optional(),
				out: z.string().optional(),
				skip: z.array(z.string()).optional(),
				volumes: z.boolean().optional(),
				textureSize: z.number().int().optional(),
				track: z.number().int().optional(),
				curve: z.string().optional(),
				key: z.number().int().optional(),
				time: z.number().optional(),
				path: z.string().optional(),
				level: z.string().optional(),
				export: z.number().int().optional(),
				category: z.string().optional(),
				filter: z.string().optional(),
				location: z.tuple([z.number(), z.number(), z.number()]).optional(),
				rotation: z.tuple([z.number(), z.number(), z.number()]).optional(),
				categories: z.record(z.string(), z.boolean()).optional(),
				levels: z.record(z.string(), z.boolean()).optional(),
				shading: z.enum(["lit", "unlit", "untextured", "wireframe"]).optional(),
				sequence: z.number().int().optional(),
				from: z.number().int().optional(),
				to: z.number().int().optional(),
				output: z.number().int().optional(),
				input: z.number().int().optional(),
				variable: z.number().int().optional(),
				window: win,
			},
		},
		(a) =>
			guarded(async () => {
				const w = target(a.window);
				const out = await evalIn(
					w,
					`const a = ${JSON.stringify(a)};
					const levelOf = (p) => map.levels.find((l) => l.path === p || l.name.toLowerCase() === String(p).toLowerCase());
					const brief = (l, x) => ({ level: l.path, export: x.export, name: x.name, class: x.class, category: x.category, edit: x.edit, location: x.location, rotation: x.rotation, info: x.info });
					switch (a.action) {
						case "list": { await map.loadList(); return map.list; }
						case "open": { if (!a.path) throw new Error("give path"); await map.open(a.path); break; }
						case "select": { const l = levelOf(a.level); if (!l) throw new Error("no such level"); await map.select({ level: l.path, export: a.export }); map.focusRequest++; break; }
						case "move": {
							const l = levelOf(a.level); if (!l) throw new Error("no such level");
							const x = l.actors.find((y) => y.export === a.export); if (!x) throw new Error("no such actor");
							await rpc("map.setTransform", { path: l.path, index: a.export, location: a.location, rotation: a.rotation, parent: x.edit === "component" ? x.parent : undefined });
							map.noteEdit(l.path);
							await map.reloadLevel(l.path);
							break;
						}
						case "show": {
							if (a.categories) map.hiddenCategories = { ...map.hiddenCategories, ...Object.fromEntries(Object.entries(a.categories).map(([k, v]) => [k, !v])) };
							if (a.levels) { const h = { ...map.hiddenLevels }; for (const [k, v] of Object.entries(a.levels)) { const l = levelOf(k); if (l) h[l.path] = !v; } map.hiddenLevels = h; }
							if (a.shading) map.shading = a.shading;
							break;
						}
						case "kismet": { const l = levelOf(a.level ?? map.path); if (!l) throw new Error("no such level"); await map.openKismet(l.path, a.sequence); return { level: l.path, sequences: map.sequences, graph: map.graph }; }
						case "link": case "unlink": {
							await map.link({ from: a.from, to: a.to, output: a.output, input: a.input, variable: a.variable, remove: a.action === "unlink" });
							return { graph: map.graph };
						}
						case "palette": { await map.loadPalette(); return map.palette.map((c) => ({ class: c.class, kind: c.kind, count: c.count })); }
						case "addNode": {
							await map.loadPalette();
							const c = map.palette.find((p) => p.class === String(a.nodeClass).toLowerCase());
							if (!c) throw new Error("no example of " + a.nodeClass + " in this folder (see palette)");
							const g = await rpc("kismet.add", { path: map.kismetLevel, sequence: map.sequence, example: { path: c.example.path, index: c.example.index }, x: a.x ?? 0, y: a.y ?? 0, originator: a.originator, object: a.object });
							map.graph = g; map.noteEdit(map.kismetLevel);
							return { added: g.added, node: g.nodes.find((n) => n.export === g.added) };
						}
						case "removeNode": await map.removeNode(a.node); return { graph: map.graph };
						case "actorPalette": {
							const l = levelOf(a.level ?? map.path); if (!l) throw new Error("no such level");
							await map.loadActorPalette(l.path);
							if (a.classes) return map.actorClasses;
							return map.actorPalette.map((c) => ({ class: c.class, category: c.category, count: c.count, inMap: c.inMap }));
						}
						case "addActor": {
							const l = levelOf(a.level ?? map.path); if (!l) throw new Error("no such level");
							let source, parent;
							if (a.newClass) {
								// Built from the class defaults below.
							} else if (a.export !== undefined) {
								const x = l.actors.find((y) => y.export === a.export); if (!x) throw new Error("no such actor in that level");
								source = { path: l.path, index: x.export };
								parent = x.edit === "component" ? x.parent : undefined;
							} else {
								await map.loadActorPalette(l.path);
								const c = map.actorPalette.find((p) => p.class === String(a.actorClass).toLowerCase());
								if (!c) throw new Error("no example of " + a.actorClass + " in this folder (see actorPalette)");
								source = { path: c.example.path, index: c.example.index };
							}
							const r = await rpc("map.addActor", { path: l.path, source, parent, class: a.newClass, mesh: a.mesh, brush: a.brush, at: a.location ? undefined : (map.viewTarget?.() ?? undefined) });
							if (a.location) await rpc("map.setTransform", { path: l.path, index: r.added, location: a.location, parent });
							map.noteEdit(l.path);
							await map.reloadLevel(l.path);
							await map.select({ level: l.path, export: r.added });
							const x = map.actor();
							return { added: r.added, warnings: r.warnings, actor: x && brief(l, x) };
						}
						case "removeActor": {
							const l = levelOf(a.level); if (!l) throw new Error("no such level");
							const x = l.actors.find((y) => y.export === a.export); if (!x) throw new Error("no such actor");
							await rpc("map.removeActor", { path: l.path, index: x.export, parent: x.edit === "component" ? x.parent : undefined });
							map.noteEdit(l.path);
							if (map.selected?.level === l.path && map.selected.export === x.export) await map.select(null);
							await map.reloadLevel(l.path);
							break;
						}
						case "heavyWalls": {
							if (a.on !== undefined) map.heavyWalls = a.on;
							const rows = [];
							for (const l of map.levels) for (const x of l.actors) if (x.canBaseOn === false) rows.push(brief(l, x));
							return { on: map.heavyWalls, count: rows.length, actors: rows.slice(0, 500) };
						}
						case "collision": {
							if (a.only !== undefined) collision.only = a.only;
							await collision.toggle(a.on ?? true);
							const d = collision.data;
							const volumes = [], none = [];
							for (const l of d?.levels ?? []) for (const it of l.items) {
								if (it.kind === "volume") volumes.push({ level: l.path, export: it.export, name: it.name, class: it.class, blocks: it.flags.blocks, hulls: it.geometry.convexCount ?? 0 });
								else { const m = d.meshes[it.meshKey]; if (m && it.flags.blocks !== "none" && m.pawns === "none") none.push({ level: l.path, export: it.export, name: it.name }); }
							}
							return { on: collision.enabled, only: collision.only, counts: collision.summary(), volumes: volumes.slice(0, 500), noBodySetup: none.slice(0, 200) };
						}
						case "brush": { const l = levelOf(a.level); if (!l) throw new Error("no such level"); return await rpc("map.brush", { path: l.path, index: a.export }); }
						case "setBrush": {
							const l = levelOf(a.level); if (!l) throw new Error("no such level");
							if (!a.brush) throw new Error("give brush");
							const ok = await collision.setBrush(l.path, a.export, a.brush);
							if (!ok) throw new Error("the shape could not be built (see the toast)");
							return await rpc("map.brush", { path: l.path, index: a.export });
						}
						case "canBaseOn": { const l = levelOf(a.level); if (!l) throw new Error("no such level"); await map.select({ level: l.path, export: a.export }); await map.setCanBaseOn(Boolean(a.value)); break; }
						case "export": {
							if (!a.out) throw new Error("give out (.glb or .fbx)");
							return await rpc("map.export", { path: map.path, out: a.out, levels: a.levels ? Object.keys(a.levels).map((k) => levelOf(k)?.path).filter(Boolean) : undefined, skip: a.skip, volumes: a.volumes, textureSize: a.textureSize });
						}
						case "matinee": { const l = levelOf(a.level ?? map.path); if (!l) throw new Error("no such level"); await map.openMatinee(l.path, a.export); return map.matinee; }
						case "setKey": { await matineeEd.setKey({ track: a.track, curve: a.curve, key: a.key }, { time: a.time, value: a.value, mode: a.mode, arrive: a.arrive, leave: a.leave, fields: a.fields, values: a.values }); return { key: map.matineeKey, timeline: map.matinee }; }
						case "addKey": { await matineeEd.addKey(a.track, a.curve, a.time, { value: a.value, mode: a.mode, values: a.values, fields: a.fields }); return { key: map.matineeKey, timeline: map.matinee }; }
						case "setInterpMode": { await matineeEd.setKey({ track: a.track, curve: a.curve, key: a.key }, { mode: a.mode }); return map.matinee; }
						case "setTangents": { await matineeEd.setKey({ track: a.track, curve: a.curve, key: a.key }, { arrive: a.arrive, leave: a.leave }); return map.matinee; }
						case "keyAtPlayhead": { matineeEd.track = a.track; await matineeEd.keyAtPlayhead(a.track); return { key: map.matineeKey, timeline: map.matinee }; }
						case "matinees": { const l = levelOf(a.level ?? map.path); if (!l) throw new Error("no such level"); await matineeEd.loadList(l.path); return matineeEd.list; }
						case "newMatinee": { const l = levelOf(a.level ?? map.path); if (!l) throw new Error("no such level"); const r = await matineeEd.create(l.path, { actor: a.actor, length: a.length, sequence: a.sequence }); if (!r) throw new Error("could not create the Matinee"); return map.matinee; }
						case "addGroup": { const r = await matineeEd.addGroup(a.name ?? "NewGroup", a.director); return { group: r?.group, timeline: map.matinee }; }
						case "removeGroup": { await matineeEd.removeGroup(a.group); return map.matinee; }
						case "setGroup": { await matineeEd.setGroup(a.group, { name: a.name, color: a.color }); return map.matinee; }
						case "addTrack": { if (!a.trackClass) throw new Error("give trackClass"); const r = await matineeEd.addTrack(a.group, a.trackClass, a.property); return { track: r?.track, timeline: map.matinee }; }
						case "removeTrack": { await matineeEd.removeTrack(a.track); return map.matinee; }
						case "setProps": { if (!a.props) throw new Error("give props"); await matineeEd.set(a.object, a.props); return map.matinee; }
						case "connect": { await matineeEd.connect(a.group, a.export, a.remove); return map.matinee; }
						case "removeKey": { await map.removeKey(a.track, a.curve, a.key); return map.matinee; }
						case "matineeTime": { map.matineeTime = a.time ?? 0; const pv = map.matineePreview; return { time: map.matineeTime, poses: Object.fromEntries(pv.poses), hidden: [...pv.hidden], tints: Object.fromEntries(pv.tints) }; }
						case "navmesh": {
							const nm = window.__tempest.navmesh;
							const op = a.navOp ?? "status";
							if (op === "show") nm.show = true;
							if (op === "hide") nm.show = false;
							if (a.colour) nm.colour = a.colour;
							await nm.load();
							if (op === "pick") { nm.pick = "faces"; nm.selected = new Set(a.faces ?? []); nm.version++; }
							if (op === "deleteFaces" || op === "cutRegion") {
								if (!a.faces?.length) throw new Error("give faces");
								if (!(await nm.edit(op === "cutRegion" ? "deleteRegion" : "deleteFaces", { faces: a.faces }))) throw new Error("the edit failed (see the toast)");
							}
							if (op === "moveVertex") {
								if (a.vertex == null || !a.location) throw new Error("give vertex and location");
								const [x, y, z] = a.location;
								if (!(await nm.moveVertex(a.vertex, [x / 100, z / 100, y / 100]))) throw new Error("the edit failed (see the toast)");
							}
							return { show: nm.show, level: nm.level, error: nm.error, colour: nm.colour, selected: [...nm.selected].slice(0, 500), actors: nm.actors.map((x) => x.info) };
						}
						case "undo": await map.undo(); break;
						case "bake": {
							const scope = a.scope ?? "edited";
							let targets;
							if (scope === "selected") { const l = levelOf(a.level ?? map.selected?.level); const e = a.export ?? map.selected?.export; if (!l || e === undefined) throw new Error("give level and export (or select an actor)"); targets = [{ path: l.path, index: e }]; }
							const levels = a.levels ? Object.keys(a.levels).filter((k) => a.levels[k]).map((k) => levelOf(k)?.path).filter(Boolean) : undefined;
							const r = await rpc("light.bake", { path: map.path, scope, targets, levels, shadowSamples: a.shadowSamples, skySamples: a.skySamples, indirectRays: a.indirectRays, albedo: a.albedo, environmentScale: a.environmentScale, compare: a.compare });
							for (const w of r.written) { map.noteEdit(w.path); await map.reloadLevel(w.path); }
							return { ...r, baked: r.baked.slice(0, 200), comparisons: r.comparisons.slice(0, 200) };
						}
						case "lightInfo": {
							const l = levelOf(a.level ?? map.selected?.level); if (!l) throw new Error("no such level");
							let e = a.export ?? map.selected?.export;
							const x = l.actors.find((y) => y.export === e);
							const part = x?.parts.find((p) => p.kind === "mesh");
							if (part) e = part.export;
							const r = await rpc("light.info", { path: l.path, index: e });
							return r;
						}
					}
					if (a.action === "actors") {
						const q = (a.filter ?? "").toLowerCase();
						const rows = [];
						for (const l of map.levels) {
							if (a.level && levelOf(a.level) !== l) continue;
							for (const x of l.actors) {
								if (a.category && x.category !== a.category) continue;
								if (q && !x.name.toLowerCase().includes(q) && !x.class.toLowerCase().includes(q)) continue;
								rows.push(brief(l, x));
							}
						}
						return { total: rows.length, actors: rows.slice(0, 500) };
					}
					return {
						map: map.path, view: map.view, error: map.error,
						levels: map.levels.map((l) => ({ path: l.path, name: l.name, actors: l.actors.length, counts: l.counts, unsaved: studio.dirty.includes(l.path), hidden: !!map.hiddenLevels[l.path] })),
						missing: map.missing,
						selected: map.selected && (() => { const l = levelOf(map.selected.level); const x = map.actor(); return l && x ? brief(l, x) : map.selected; })(),
						hiddenCategories: Object.keys(map.hiddenCategories).filter((k) => map.hiddenCategories[k]),
						shading: map.shading,
					};`,
				);
				return text(out);
			}),
	);

	server.registerTool(
		"studio_mod",
		{
			title: "Mod cooker",
			description:
				"The mod cooker of a window: what the mod holds and building it. `status`: the game files it changes (target, sources with their origin unsaved/saved/copy, the choice: included, delta or whole, source), each file's preview (objects or rows changed/added/removed) once `preview` ran, extra files, the project's mod-files, details, problems that block the build, the signing key, the last build and the build log. `refresh`: look for new changes. `preview`: compute what each delta holds. `set`: change details (`meta`: name, version (semver, checked like the launcher: 1.2.3 or 1.2.3-beta.1), id (sets a custom id; 2–64 of a-z 0-9 . _ -), authors [{name, link}], readme (Markdown), dependencies (one per line, `id` + optional requirement like >=1.0.0, ^2, ~1.2; `tempest` is the launcher), summary (≤ 140), tags, repository (source link), patches (comma-separated) or patchFrom/patchTo, changelog, compression small|fast); problems lists what blocks the build (invalid id, version, dependency lines, icon), per-file `choices`, and `extras` ([{src, dest}] files to ship, dest inside the mod). `build`: write, pack and sign the .tempest (in a project to builds/<id>-<version>.tempest, else to `out`, which is required then); may ask before replacing an existing build (answer with studio_confirm). `install`: install the last build into the window's game folder (may ask first). Publishing is left to the user.",
			inputSchema: {
				action: z.enum(["status", "refresh", "preview", "set", "build", "install"]),
				meta: z.record(z.string(), z.unknown()).optional(),
				choices: z.array(choice).optional(),
				extras: z.array(z.object({ src: z.string(), dest: z.string() })).optional(),
				out: z
					.string()
					.optional()
					.describe("build: the .tempest to write (outside a project)"),
				window: win,
			},
		},
		(a) =>
			guarded(async () => {
				const w = target(a.window);
				const outcome = await evalIn(
					w,
					`const a = ${JSON.stringify(a)};
					let outcome = { status: "done" };
					if (a.action === "refresh") await cooker.refresh();
					else if (a.action === "preview") { await cooker.refresh(); await cooker.preview(); }
					else if (a.action === "set") {
						const m = a.meta ?? {};
						for (const [k, v] of Object.entries(m)) {
							if (k === "id") { cooker.meta.id = String(v); cooker.meta.idCustom = String(v).trim() !== ""; }
							else if (k in cooker.meta && k !== "idCustom" && k !== "icon") cooker.meta[k] = v;
							else throw new Error("unknown detail " + k);
						}
						for (const c of a.choices ?? []) {
							const { target, ...patch } = c;
							if (!cooker.entries.some((e) => e.target === target)) throw new Error("not in the mod: " + target);
							cooker.setChoice(target, patch);
						}
						if (a.extras) cooker.extras = a.extras;
					} else if (a.action === "build") {
						if (!cooker.projectDir && !a.out) throw new Error("outside a project, give out (the .tempest to write)");
						await cooker.refresh();
						if (!cooker.canCook) throw new Error("cannot build: " + (cooker.problems.join(" ") || "tempest-cli is not available"));
						outcome = await watch(cooker.cook(a.out), 600000);
					} else if (a.action === "install") {
						if (!cooker.cooked) throw new Error("build the mod first");
						outcome = await watch(cooker.install(false), 600000);
					}
					return {
						...outcome,
						mod: {
							id: cooker.modId, idValid: cooker.idValid, versionValid: cooker.versionValid,
							details: JSON.parse(JSON.stringify(cooker.meta)),
							dependencies: cooker.dependencies,
							projectDir: cooker.projectDir,
							projectFiles: cooker.projectFiles,
							entries: cooker.shown.map((e) => ({
								target: e.target, rel: e.rel, kind: e.kind,
								sources: e.sources.map((s) => ({ id: s.id, origin: s.origin, source: s.source, base: s.base.kind, script: s.script })),
								choice: e.choice,
								preview: e.chosen ? cooker.previews[e.chosen.id] : undefined,
							})),
							extras: JSON.parse(JSON.stringify(cooker.extras)),
							problems: cooker.problems,
							account: cooker.account ? { signingKey: cooker.account.signing.keyId, hasSigningKey: cooker.account.signing.hasKey, repository: cooker.account.repo.url, hasApiKey: cooker.account.repo.hasApiKey } : null,
							canBuild: cooker.canCook,
							signing: cooker.cli ? { cliFound: cooker.cli.available, hasKey: cooker.cli.hasKey } : null,
							built: cooker.cooked,
							busy: cooker.busy,
							log: cooker.log.slice(-30),
						},
					};`,
				);
				return text(outcome);
			}),
	);

	server.registerTool(
		"studio_screenshot",
		{
			title: "Screenshot",
			description: "A PNG of the window as it is now.",
			inputSchema: { window: win },
		},
		({ window }) =>
			guarded(async () => {
				const image = await target(window).webContents.capturePage();
				return {
					content: [
						{
							type: "image" as const,
							data: image.toPNG().toString("base64"),
							mimeType: "image/png",
						},
					],
				};
			}),
	);

	server.registerTool(
		"studio_new_window",
		{
			title: "New window",
			description:
				"Open another Studio window (its own backend), optionally on a workspace folder.",
			inputSchema: { dir: z.string().optional() },
		},
		({ dir }) =>
			guarded(() => {
				if (!createWindowHook) throw new Error("window creation is not available");
				const w = createWindowHook(dir ? { dir } : {});
				return Promise.resolve(text({ window: w.webContents.id }));
			}),
	);

	return server;
}

// ----- HTTP ---------------------------------------------------------------------

const transports = new Map<string, StreamableHTTPServerTransport>();

function readBody(req: http.IncomingMessage): Promise<unknown> {
	return new Promise((resolve, reject) => {
		const chunks: Buffer[] = [];
		req.on("data", (c: Buffer) => chunks.push(c));
		req.on("end", () => {
			const raw = Buffer.concat(chunks).toString("utf8");
			if (!raw) {
				resolve(undefined);
				return;
			}
			try {
				resolve(JSON.parse(raw));
			} catch (error) {
				reject(error);
			}
		});
		req.on("error", reject);
	});
}

async function serve(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
	const url = new URL(req.url ?? "/", "http://localhost");
	if (url.pathname !== "/mcp") {
		res.writeHead(404).end();
		return;
	}
	const auth = req.headers.authorization ?? "";
	if (auth !== `Bearer ${info.token}`) {
		res.writeHead(401, { "content-type": "application/json" }).end(
			JSON.stringify({ error: "bad token (see the Studio's MCP dialog)" }),
		);
		return;
	}
	const sessionId = req.headers["mcp-session-id"];
	const transport = typeof sessionId === "string" ? transports.get(sessionId) : undefined;
	if (!transport) {
		const body = req.method === "POST" ? await readBody(req) : undefined;
		if (req.method !== "POST" || !isInitializeRequest(body)) {
			res.writeHead(400, { "content-type": "application/json" }).end(
				JSON.stringify({ error: "no session: send an initialize request first" }),
			);
			return;
		}
		const created = new StreamableHTTPServerTransport({
			sessionIdGenerator: () => randomUUID(),
			onsessioninitialized: (id) => {
				transports.set(id, created);
			},
		});
		created.onclose = () => {
			if (created.sessionId) transports.delete(created.sessionId);
		};
		await buildServer().connect(created);
		await created.handleRequest(req, res, body);
		return;
	}
	await transport.handleRequest(
		req,
		res,
		req.method === "POST" ? await readBody(req) : undefined,
	);
}

/** Start listening (localhost only). */
export function startMcp(): void {
	// The token is kept in the app's data folder so a client configured once
	// (`claude mcp add …`) keeps working across restarts; delete the file to rotate it.
	const tokenPath = path.join(app.getPath("userData"), "mcp-token");
	let token = "";
	try {
		token = fs.readFileSync(tokenPath, "utf8").trim();
	} catch {
		// first start
	}
	if (!/^[0-9a-f]{32,}$/.test(token)) {
		token = randomBytes(24).toString("hex");
		try {
			fs.mkdirSync(path.dirname(tokenPath), { recursive: true });
			fs.writeFileSync(tokenPath, token, { mode: 0o600 });
		} catch (error) {
			console.error("[mcp] could not write", tokenPath, error);
		}
	}
	const configPath = path.join(app.getPath("userData"), "mcp.json");
	const server = http.createServer((req, res) => {
		serve(req, res).catch((error: unknown) => {
			console.error("[mcp]", error);
			if (!res.headersSent) res.writeHead(500).end();
		});
	});
	server.on("error", (error) => {
		console.error(`[mcp] cannot listen on ${PORT}: ${error.message}`);
		info = { ...info, running: false };
	});
	server.listen(PORT, "127.0.0.1", () => {
		const url = `http://127.0.0.1:${PORT}/mcp`;
		info = { url, token, configPath, running: true };
		try {
			fs.writeFileSync(configPath, JSON.stringify({ url, token, pid: process.pid }, null, 2));
		} catch (error) {
			console.error("[mcp] could not write", configPath, error);
		}
		console.log(`[mcp] listening on ${url}`);
	});
	app.on("will-quit", () => {
		server.close();
		try {
			fs.rmSync(configPath, { force: true });
		} catch {
			// ignore
		}
	});
}

handle("mcp:info", () => mcpInfo());
