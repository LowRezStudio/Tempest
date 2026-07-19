const eio = () => (window as any).electronAPI!;

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
	};
}

// ---- @tauri-apps/api/app ----

export function getVersion(): Promise<string> {
	return eio().invoke("app:version");
}

// ---- @tauri-apps/plugin-fs ----

export function exists(p: string): Promise<boolean> {
	return eio().invoke("fs:exists", { path: p });
}

export function readDir(
	p: string,
): Promise<{ name: string; isDirectory: boolean; isFile: boolean; isSymlink: boolean }[]> {
	return eio().invoke("fs:read-dir", { path: p });
}

export function stat(
	p: string,
): Promise<{ size: number; isDirectory: boolean; isFile: boolean; isSymlink: boolean }> {
	return eio().invoke("fs:stat", { path: p });
}

export function remove(p: string, options?: { recursive?: boolean }): Promise<void> {
	return eio().invoke("fs:remove", { path: p, options });
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
