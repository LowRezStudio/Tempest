// MCP server of the Studio: agents drive the running app over Streamable
// HTTP on localhost. Tools call the `upk serve` backend of a window
// (`upk_call`, every method of the IPC contract), read and change what the
// window shows (workspace, package, selection, assembly table, view), run
// the Studio's commands, answer its confirmation dialogs, work with mod
// projects and the mod cooker, evaluate code against the renderer stores
// (`studio_eval`) and take screenshots.
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
 * `nav`, `cooker`) and `watch` in scope. The returned value comes back as JSON. */
async function evalIn(win: BrowserWindow, code: string): Promise<unknown> {
	const wrapped = `(async () => {
		const api = window.__tempest;
		if (!api) throw new Error("the renderer is not ready");
		const { studio, assembly, rpc, session, ui, tabs, commands, nav, cooker } = api;
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
				"Call any method of the window's `upk serve` backend (the UpkMethods of electron/src/ipc/contract.ts): workspace.search, package.open, object.get, prop.set, object.list, object.duplicate, texture.export, mesh.export, material.preview, particle.system, swf.info, mcts.rows, mcts.set, mcts.addRow, mcts.usages, mod.changes, mod.preview, delta.create, twrk.read, … Binary payloads come back as base64. Edits made here are not reflected in the window until it reloads (use studio_open_package / studio_select to refresh); prefer studio_eval with the stores (studio.setProp…) when the window should follow, so the edit also reaches undo, the project and the cooker.",
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
			description: "Show the Packages view, the Assembly view or the mod cooker (mod).",
			inputSchema: { mode: z.enum(["packages", "assembly", "mod"]), window: win },
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
				"Run JavaScript in a window with the renderer stores in scope: `studio` (packages, state.svelte.ts), `assembly` (assembly.svelte.ts), `rpc` (typed backend calls), `session` (projects and workspace files, session.svelte.ts), `cooker` (mod cooker, cooker.svelte.ts), `commands` (command registry), `ui` (dialogs, ui.asking = pending confirmation), `tabs`, `nav`; plus `watch(promise, ms)` which returns early with the question when the action asks for a confirmation. The code is the body of an async function and its return value comes back as JSON. Example: `return (await rpc('workspace.search', { query: 'zhin', limit: 5 })).results;`",
			inputSchema: { code: z.string(), window: win },
		},
		({ code, window }) => guarded(async () => text(await evalIn(target(window), code))),
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
		"studio_mod",
		{
			title: "Mod cooker",
			description:
				"The mod cooker of a window: what the mod holds and building it. `status`: the game files it changes (target, sources with their origin unsaved/saved/copy, the choice: included, delta or whole, source), each file's preview (objects or rows changed/added/removed) once `preview` ran, extra files, the project's mod-files, details, problems that block the build, the signing key, the last build and the build log. `refresh`: look for new changes. `preview`: compute what each delta holds. `set`: change details (`meta`: name, version (semver), id (sets a custom id), authors [{name, link}], readme (description), dependencies (one per line), patchFrom, patchTo, summary, changelog, tags), per-file `choices`, and `extras` ([{src, dest}] files to ship, dest inside the mod). `build`: write, pack and sign the .tempest (in a project to builds/<id>-<version>.tempest, else to `out`, which is required then); may ask before replacing an existing build (answer with studio_confirm). `install`: install the last build into the window's game folder (may ask first). Publishing is left to the user.",
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
