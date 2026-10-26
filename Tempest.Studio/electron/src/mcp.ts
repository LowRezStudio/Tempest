// MCP server of the Studio: agents drive the running app over Streamable
// HTTP on localhost. Tools call the `upk serve` backend of a window
// (`upk_call`, every method of the IPC contract), read and change what the
// window shows (workspace, package, selection, assembly table), evaluate
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
 * with `studio`, `assembly` and `rpc` in scope. The returned value comes back as JSON. */
async function evalIn(win: BrowserWindow, code: string): Promise<unknown> {
	const wrapped = `(async () => {
		const api = window.__tempest;
		if (!api) throw new Error("the renderer is not ready");
		const { studio, assembly, rpc } = api;
		const __result = await (async () => { ${code} })();
		return JSON.stringify(__result === undefined ? null : __result);
	})()`;
	const text = (await win.webContents.executeJavaScript(wrapped, true)) as string;
	return JSON.parse(text);
}

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
				"What a window shows: workspace, package, selected export, dirty packages, undo labels, assembly table.",
			inputSchema: { window: win },
		},
		({ window }) => guarded(async () => text(await stateOf(target(window)))),
	);

	server.registerTool(
		"upk_call",
		{
			title: "Call the backend",
			description:
				"Call any method of the window's `upk serve` backend (the UpkMethods of electron/src/ipc/contract.ts): workspace.search, package.open, object.parse, prop.set, texture.export, mesh.export, mcts.rows, mcts.set, … Binary payloads come back as base64. Edits made here are not reflected in the window until it reloads (use studio_open_package / studio_select to refresh).",
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
			description: "Show the Packages editor or the Assembly editor.",
			inputSchema: { mode: z.enum(["packages", "assembly"]), window: win },
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
				"Run JavaScript in a window with `studio` (src/lib/studio/state.svelte.ts), `assembly` (assembly.svelte.ts) and `rpc` (typed backend calls) in scope; the code is the body of an async function and its return value comes back as JSON. Example: `return (await rpc('workspace.search', { query: 'zhin', limit: 5 })).results;`",
			inputSchema: { code: z.string(), window: win },
		},
		({ code, window }) => guarded(async () => text(await evalIn(target(window), code))),
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
