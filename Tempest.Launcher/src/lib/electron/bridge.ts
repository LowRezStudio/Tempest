const eio = () => (window as any).electronAPI!;

type FsResult<T> = { ok: true; data: T } | { ok: false; error: string };

async function unwrap<T>(p: Promise<FsResult<T>>): Promise<T> {
	const result = await p;
	if (!result.ok) return Promise.reject(result.error);
	return result.data;
}

class MiniEmitter {
	private _listeners = new Map<string, Set<Function>>();

	on(event: string, cb: Function): this {
		if (!this._listeners.has(event)) this._listeners.set(event, new Set());
		this._listeners.get(event)!.add(cb);
		return this;
	}

	removeListener(event: string, cb: Function): this {
		this._listeners.get(event)?.delete(cb);
		return this;
	}

	emit(event: string, ...args: unknown[]) {
		for (const cb of this._listeners.get(event) ?? []) cb(...args);
	}
}

// ---- @tauri-apps/plugin-shell ----

export type SpawnOptions = { env?: Record<string, string> };

class ElectronChild {
	pid: number;
	private _emitter = new MiniEmitter();

	constructor(pid: number) {
		this.pid = pid;
	}

	on(event: string, cb: Function): this {
		this._emitter.on(event, cb);
		return this;
	}

	async kill(): Promise<void> {
		await eio().invoke("shell:kill", { pid: this.pid });
	}

	async write(data: string): Promise<void> {
		await eio().invoke("shell:stdin-write", { pid: this.pid, data });
	}

	_handleClose(payload: { code: number | null; signal: number | null }) {
		this._emitter.emit("close", payload);
	}

	_handleError(error: string) {
		this._emitter.emit("error", error);
	}
}

export type Child = ElectronChild;

export class Command {
	stdout = new MiniEmitter();
	stderr = new MiniEmitter();
	private _emitter = new MiniEmitter();

	private constructor(
		private program: string,
		private args: string[],
		private options?: SpawnOptions,
		private _sidecar = false,
	) {}

	static create(program: string, args?: string[], options?: SpawnOptions): Command {
		return new Command(program, args ?? [], options);
	}

	static sidecar(name: string, args?: string[], options?: SpawnOptions): Command {
		return new Command(name, args ?? [], options, true);
	}

	on(event: string, cb: Function): this {
		this._emitter.on(event, cb);
		return this;
	}

	removeListener(event: string, cb: Function): this {
		this._emitter.removeListener(event, cb);
		return this;
	}

	async spawn(): Promise<ElectronChild> {
		const { pid } = await eio().invoke("shell:spawn", {
			program: this.program,
			args: this.args,
			options: this.options,
			sidecar: this._sidecar,
		});

		const child = new ElectronChild(pid);

		eio().on(`shell:process:${pid}:stdout`, (data: string) => {
			this.stdout.emit("data", data);
		});
		eio().on(`shell:process:${pid}:stderr`, (data: string) => {
			this.stderr.emit("data", data);
		});
		eio().on(
			`shell:process:${pid}:close`,
			(payload: { code: number | null; signal: number | null }) => {
				this._emitter.emit("close", payload);
				child._handleClose(payload);
			},
		);
		eio().on(`shell:process:${pid}:error`, (error: string) => {
			this._emitter.emit("error", error);
			child._handleError(error);
		});

		return child;
	}

	execute(): Promise<{
		code: number | null;
		signal: number | null;
		stdout: string;
		stderr: string;
	}> {
		return eio().invoke("shell:execute", {
			program: this.program,
			args: this.args,
			options: this.options,
			sidecar: this._sidecar,
		});
	}
}

// ---- @tauri-apps/api/core ----

export function invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
	return eio().invoke(cmd, args);
}

// ---- @tauri-apps/api (path module) ----
// ponytail: path ops go through IPC for correctness across platforms

export const path = {
	join: (...parts: string[]): Promise<string> => eio().invoke("path:join", { paths: parts }),
	dirname: (p: string): Promise<string> => eio().invoke("path:dirname", { path: p }),
};

// ---- @tauri-apps/api/path ----

export function appConfigDir(): Promise<string> {
	return eio().invoke("path:app-config-dir");
}

export function homeDir(): Promise<string> {
	return eio().invoke("path:home-dir");
}

export function resolveResource(...paths: string[]): Promise<string> {
	return eio().invoke("path:resolve-resource", { paths });
}

// ---- @tauri-apps/api/window ----
// ponytail: drag-drop uses DOM events + webUtils.getPathForFile from preload

export function getCurrentWindow() {
	return {
		onCloseRequested(
			handler: (event: { preventDefault: () => void }) => Promise<void>,
		): Promise<() => void> {
			(window as any).__closeHook = async () => {
				let prevented = false;
				await handler({
					preventDefault: () => {
						prevented = true;
					},
				});
				return !prevented;
			};
			return Promise.resolve(() => {
				(window as any).__closeHook = undefined;
			});
		},

		onDragDropEvent(
			handler: (event: { payload: { type: string; paths: string[] } }) => void,
		): Promise<() => void> {
			const onEnter = (e: DragEvent) => {
				e.preventDefault();
				handler({ payload: { type: "enter", paths: [] } });
			};
			const onOver = (e: DragEvent) => {
				e.preventDefault();
				handler({ payload: { type: "over", paths: [] } });
			};
			const onLeave = () => {
				handler({ payload: { type: "leave", paths: [] } });
			};
			const onDrop = (e: DragEvent) => {
				e.preventDefault();
				const files = e.dataTransfer?.files;
				const paths: string[] = [];
				if (files) {
					for (let i = 0; i < files.length; i++) {
						paths.push(eio().getPathForFile(files[i]));
					}
				}
				handler({ payload: { type: "drop", paths } });
			};

			document.addEventListener("dragenter", onEnter);
			document.addEventListener("dragover", onOver);
			document.addEventListener("dragleave", onLeave);
			document.addEventListener("drop", onDrop);

			return Promise.resolve(() => {
				document.removeEventListener("dragenter", onEnter);
				document.removeEventListener("dragover", onOver);
				document.removeEventListener("dragleave", onLeave);
				document.removeEventListener("drop", onDrop);
			});
		},

		async destroy(): Promise<void> {
			await eio().invoke("window:destroy");
		},

		async hide(): Promise<void> {
			await eio().invoke("window:hide");
		},

		async show(): Promise<void> {
			await eio().invoke("window:show");
		},

		async setFocus(): Promise<void> {
			await eio().invoke("window:set-focus");
		},

		async unminimize(): Promise<void> {
			await eio().invoke("window:unminimize");
		},
	};
}

// ---- @tauri-apps/api/app ----

export function getVersion(): Promise<string> {
	return eio().invoke("app:version");
}

// ---- @tauri-apps/api/menu ----
// ponytail: actions stay in the renderer; only serializable items cross IPC

export interface MenuItemOptions {
	id?: string;
	text: string;
	enabled?: boolean;
	action?: (id: string) => void;
}

export interface PredefinedMenuItemOptions {
	text?: string;
	item: string;
}

type NativeMenuItemOptions = MenuItemOptions | PredefinedMenuItemOptions;

type SerializedMenuItem =
	| { kind: "separator" }
	| { kind: "item"; id: string; text: string; enabled: boolean };

let menuSeq = 0;
const menuItemActions = new Map<string, (id: string) => void>();

function serializeMenuItems(items: NativeMenuItemOptions[]): SerializedMenuItem[] {
	return items.map((item) => {
		if ("item" in item) {
			if (item.item !== "Separator") {
				throw new Error(`Unsupported predefined menu item: ${item.item}`);
			}
			return { kind: "separator" };
		}
		const id = item.id ?? `item-${menuSeq++}`;
		if (item.action) menuItemActions.set(id, item.action);
		return { kind: "item", id, text: item.text, enabled: item.enabled ?? true };
	});
}

export class Menu {
	id: string;
	_items: SerializedMenuItem[];

	private constructor(id: string, items: SerializedMenuItem[]) {
		this.id = id;
		this._items = items;
	}

	static new(opts?: { id?: string; items?: NativeMenuItemOptions[] }): Promise<Menu> {
		return Promise.resolve(
			new Menu(opts?.id ?? `menu-${menuSeq++}`, serializeMenuItems(opts?.items ?? [])),
		);
	}
}

// ---- @tauri-apps/api/tray ----

export type MouseButton = "Left" | "Right" | "Middle";
export type MouseButtonState = "Up" | "Down";

export type TrayIconEvent = {
	type: "Click" | "DoubleClick" | "Enter" | "Move" | "Leave";
	id: string;
	button?: MouseButton;
	buttonState?: MouseButtonState;
	position?: { x: number; y: number };
	rect?: { position: { x: number; y: number }; size: { width: number; height: number } };
};

export interface TrayIconOptions {
	id?: string;
	menu?: Menu;
	tooltip?: string;
	showMenuOnLeftClick?: boolean;
	action?: (event: TrayIconEvent) => void;
}

const trayActions = new Map<string, (event: TrayIconEvent) => void>();

let trayEventsBound = false;

function bindTrayEvents(): void {
	if (trayEventsBound) return;
	trayEventsBound = true;
	eio().on("tray:menu-event", ({ id }: { id: string }) => {
		menuItemActions.get(id)?.(id);
	});
	eio().on("tray:event", ({ id, event }: { id: string; event: TrayIconEvent }) => {
		trayActions.get(id)?.(event);
	});
}

export class TrayIcon {
	id: string;

	private constructor(id: string) {
		this.id = id;
	}

	static async new(options?: TrayIconOptions): Promise<TrayIcon> {
		bindTrayEvents();
		const id = options?.id ?? `tray-${menuSeq++}`;
		if (options?.action) trayActions.set(id, options.action);
		await eio().invoke("tray:new", {
			id,
			tooltip: options?.tooltip,
			showMenuOnLeftClick: options?.showMenuOnLeftClick ?? true,
		});
		const tray = new TrayIcon(id);
		if (options?.menu) await tray.setMenu(options.menu);
		return tray;
	}

	static async removeById(id: string): Promise<void> {
		trayActions.delete(id);
		await eio().invoke("tray:remove", { id });
	}

	async setMenu(menu: Menu | null): Promise<void> {
		await eio().invoke("tray:set-menu", { id: this.id, items: menu?._items ?? [] });
	}

	async setTooltip(tooltip: string | null): Promise<void> {
		await eio().invoke("tray:set-tooltip", { id: this.id, tooltip });
	}
}

// ---- @tauri-apps/plugin-fs ----

export function exists(p: string): Promise<boolean> {
	return eio().invoke("fs:exists", { path: p });
}

export function readDir(
	p: string,
): Promise<{ name: string; isDirectory: boolean; isFile: boolean; isSymlink: boolean }[]> {
	return unwrap(eio().invoke("fs:read-dir", { path: p }));
}

export function stat(
	p: string,
): Promise<{ size: number; isDirectory: boolean; isFile: boolean; isSymlink: boolean }> {
	return unwrap(eio().invoke("fs:stat", { path: p }));
}

export function remove(p: string, options?: { recursive?: boolean }): Promise<void> {
	return unwrap(eio().invoke("fs:remove", { path: p, options })) as Promise<void>;
}

// ---- @tauri-apps/plugin-dialog ----

export function open(options?: {
	directory?: boolean;
	multiple?: boolean;
	filters?: { name: string; extensions: string[] }[];
	title?: string;
}): Promise<string | string[] | null> {
	return eio().invoke("dialog:open", { options });
}

// ---- @tauri-apps/plugin-opener ----

export function openPath(p: string): Promise<void> {
	return eio().invoke("opener:open-path", { path: p });
}

export function openUrl(url: string): Promise<void> {
	return eio().invoke("opener:open-url", { url });
}

// ---- @tauri-apps/plugin-os ----
// injected as non-writable constants from main process via executeJavaScript

export function platform(): string {
	return (window as any).__os?.platform ?? "";
}

export function arch(): string {
	return (window as any).__os?.arch ?? "";
}

export function type(): string {
	return (window as any).__os?.type ?? "";
}

export function version(): string {
	return (window as any).__os?.version ?? "";
}

// ---- @tauri-apps/plugin-http ----

export const fetch = ((input: RequestInfo | URL, init?: RequestInit) =>
	window.fetch(input, init)) as typeof window.fetch;

// ---- @tauri-apps/plugin-updater ----

class ElectronUpdate {
	constructor(private _version: string) {}

	get version(): string {
		return this._version;
	}

	async downloadAndInstall(
		onEvent: (event: {
			event: string;
			data?: { contentLength?: number; chunkLength?: number };
		}) => void,
	): Promise<void> {
		const unsub = eio().on(
			"updater:download-progress",
			(data: { chunkLength: number; contentLength: number }) => {
				if (data.contentLength) {
					onEvent({ event: "Started", data: { contentLength: data.contentLength } });
				}
				onEvent({ event: "Progress", data: { chunkLength: data.chunkLength } });
			},
		);

		try {
			await eio().invoke("updater:download");
			onEvent({ event: "Finished" });
		} finally {
			unsub();
		}
	}
}

export async function check(): Promise<ElectronUpdate | null> {
	const result = (await eio().invoke("updater:check")) as {
		available: boolean;
		version?: string;
		error?: string;
	};
	if (result.error) throw new Error(result.error);
	return result.available && result.version ? new ElectronUpdate(result.version) : null;
}

export type Update = ElectronUpdate;

// ---- @tauri-apps/plugin-sql ----

export interface QueryResult {
	rowsAffected: number;
	lastInsertId?: number;
}

export default class Database {
	path: string;

	constructor(path: string) {
		this.path = path;
	}

	static async load(path: string): Promise<Database> {
		await eio().invoke("sql:load", { path });
		return new Database(path);
	}

	static get(path: string): Database {
		return new Database(path);
	}

	execute(query: string, bindValues?: unknown[]): Promise<QueryResult> {
		return eio().invoke("sql:execute", {
			db: this.path,
			query,
			values: bindValues ?? [],
		}) as Promise<QueryResult>;
	}

	select<T>(query: string, bindValues?: unknown[]): Promise<T> {
		return eio().invoke("sql:select", {
			db: this.path,
			query,
			values: bindValues ?? [],
		}) as Promise<T>;
	}

	async close(db?: string): Promise<boolean> {
		await eio().invoke("sql:close", { db: db ?? this.path });
		return true;
	}
}
