// Typed renderer API for Electron IPC; filesystem, process and window operations stay in the host.
import type {
	ContextMenuAction,
	ContextMenuOptions,
	IpcChannel,
	IpcContract,
	IpcEventChannel,
	IpcEvents,
	PreloadApi,
} from "$ipc/contract";

export type { ContextMenuAction, ContextMenuOptions };

const eio = (): PreloadApi => {
	if (!window.electronAPI) {
		throw new Error("electronAPI unavailable (not running under Electron)");
	}
	return window.electronAPI;
};

// ---- IPC invoke / events ----

export function invoke<K extends IpcChannel>(
	channel: K,
	...args: IpcContract[K]["args"] extends void ? [] : [IpcContract[K]["args"]]
): Promise<IpcContract[K]["result"]> {
	return eio().invoke(channel, ...args);
}

export function listen<K extends IpcEventChannel>(
	event: K,
	handler: (payload: IpcEvents[K]) => void,
): Promise<() => void> {
	return Promise.resolve(eio().on(event, handler));
}

// ---- Shell: child processes ----

export type SpawnOptions = { env?: Record<string, string> };

type ClosePayload = { code: number | null; signal: string | null };

/** Typed "data" stream for stdout/stderr of a {@link Command}. */
class CommandStream<O> {
	private _emitter = new MiniEmitter();

	on(_event: "data", cb: (data: O) => void): this {
		this._emitter.on("data", cb);
		return this;
	}

	removeListener(_event: "data", cb: (data: O) => void): this {
		this._emitter.removeListener("data", cb);
		return this;
	}

	emit(data: O) {
		this._emitter.emit("data", data);
	}
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

export class Command<O = string> {
	stdout = new CommandStream<O>();
	stderr = new CommandStream<O>();
	private _emitter = new MiniEmitter();

	private constructor(
		private program: string,
		private args: string[],
		private options?: SpawnOptions,
		private _sidecar = false,
	) {}

	static create<O = string>(
		program: string,
		args?: string[],
		options?: SpawnOptions,
	): Command<O> {
		return new Command(program, args ?? [], options);
	}

	static sidecar<O = string>(name: string, args?: string[], options?: SpawnOptions): Command<O> {
		return new Command(name, args ?? [], options, true);
	}

	/**
	 * Rebuild the event plumbing for a child that outlived this page (a Vite
	 * reload or renderer crash leaves the host process running). The command has
	 * no program: it only routes the host's `shell:process` stream for the pid.
	 * @param pid - Host child id from `shell:list`.
	 * @returns The reattached command and its child.
	 */
	static attach(pid: number): { command: Command; child: Child } {
		const command = new Command("", []);
		return { command, child: command.listen(pid) };
	}

	on(event: "close", cb: (payload: ClosePayload) => void): this;
	on(event: "error", cb: (error: string) => void): this;
	on(event: string, cb: Function): this {
		this._emitter.on(event, cb);
		return this;
	}

	removeListener(event: string, cb: Function): this {
		this._emitter.removeListener(event, cb);
		return this;
	}

	async spawn(): Promise<Child> {
		const { pid } = await eio().invoke("shell:spawn", {
			program: this.program,
			args: this.args,
			options: this.options,
			sidecar: this._sidecar,
		});

		return this.listen(pid);
	}

	private listen(pid: number): Child {
		const child = new Child(pid);

		// One demultiplexed event stream for all spawned children. Remove all
		// four listeners once the process is over; otherwise every launch leaks
		// listeners (and the buffers they retain) for the lifetime of the
		// renderer. Only on close: Node emits close after both a normal exit and
		// a spawn error, and callers' close handlers (e.g. the process list
		// cleanup) must still see it.
		const unlisten = new Set<() => void>();
		const cleanup = () => {
			for (const fn of unlisten) fn();
			unlisten.clear();
		};

		const onProcessEvent = (event: {
			pid: number;
			type: string;
			data?: string;
			code?: number | null;
			signal?: string | null;
			message?: string;
		}) => {
			if (event.pid !== pid) return;
			switch (event.type) {
				case "stdout": {
					this.stdout.emit(event.data as O);
					break;
				}
				case "stderr": {
					this.stderr.emit(event.data as O);
					break;
				}
				case "close": {
					const payload = { code: event.code ?? null, signal: event.signal ?? null };
					this._emitter.emit("close", payload);
					child._handleClose(payload);
					cleanup();
					break;
				}
				case "error": {
					const message = event.message ?? "unknown error";
					this._emitter.emit("error", message);
					child._handleError(message);
					break;
				}
			}
		};

		unlisten.add(eio().on("shell:process", onProcessEvent));

		return child;
	}

	/**
	 * Run to completion and collect the output.
	 * @param onStderr - Hears stderr as it arrives, before the result (which still holds all of it).
	 */
	async execute(onStderr?: (data: string) => void): Promise<{
		code: number | null;
		signal: string | null;
		stdout: string;
		stderr: string;
	}> {
		const request = {
			program: this.program,
			args: this.args,
			options: this.options,
			sidecar: this._sidecar,
		};
		if (!onStderr) return eio().invoke("shell:execute", request);
		const streamId = crypto.randomUUID();
		const unlisten = eio().on("shell:execute-stderr", ({ id, data }) => {
			if (id === streamId) onStderr(data);
		});
		try {
			return await eio().invoke("shell:execute", { ...request, streamId });
		} finally {
			unlisten();
		}
	}
}

export class Child {
	pid: number;
	private _emitter = new MiniEmitter();

	constructor(pid: number) {
		this.pid = pid;
	}

	on(event: "close", cb: (payload: ClosePayload) => void): this;
	on(event: "error", cb: (error: string) => void): this;
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

	// Write a command with exactly one trailing newline.
	sendLine(line: string): Promise<void> {
		let end = line.length;
		while (end > 0 && (line[end - 1] === "\n" || line[end - 1] === "\r")) end--;
		return this.write(`${line.slice(0, end)}\n`);
	}

	_handleClose(payload: ClosePayload) {
		this._emitter.emit("close", payload);
	}

	_handleError(error: string) {
		this._emitter.emit("error", error);
	}
}

// ---- Path helpers (over IPC for cross-platform correctness) ----

export function join(...paths: string[]): Promise<string> {
	return eio().invoke("path:join", { paths });
}

export function dirname(p: string): Promise<string> {
	return eio().invoke("path:dirname", { path: p });
}

export function resolveResource(...paths: string[]): Promise<string> {
	return eio().invoke("path:resolve-resource", { paths });
}

// ---- Window ----
// Drag-drop uses DOM events + webUtils.getPathForFile from preload; closing
// is decided host-side via the close-behavior flag (see tray.svelte.ts).

/**
 * Registers a drag-and-drop listener that reports dropped file paths.
 * @param handler Called with `{type: "enter"|"over"|"leave"|"drop", paths, target}`, the element
 * under the pointer as `target`. Returning `false` for "enter" or "over" shows that no drop is taken there.
 * @returns A promise resolving to an unlisten function.
 */
export function onDragDropEvent(
	handler: (event: {
		payload: { type: string; paths: string[]; target: Element | null };
	}) => boolean | void,
): Promise<() => void> {
	const targetOf = (e: DragEvent) => (e.target instanceof Element ? e.target : null);
	// Always prevented, even where nothing is taken: a file dropped unhandled would replace the page.
	const onHover = (type: "enter" | "over") => (e: DragEvent) => {
		e.preventDefault();
		const takes = handler({ payload: { type, paths: [], target: targetOf(e) } });
		if (takes === false && e.dataTransfer) e.dataTransfer.dropEffect = "none";
	};
	const onEnter = onHover("enter");
	const onOver = onHover("over");
	const onLeave = (e: DragEvent) => {
		// Moving between elements leaves one for the next; only leaving the window counts.
		if (e.relatedTarget) return;
		handler({ payload: { type: "leave", paths: [], target: null } });
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
		handler({ payload: { type: "drop", paths, target: targetOf(e) } });
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
}

export async function destroyWindow(): Promise<void> {
	await eio().invoke("window:destroy");
}

// Show the native text menu, returning the console action or null on dismissal/native edits.
export function showContextMenu(options: ContextMenuOptions): Promise<ContextMenuAction | null> {
	return eio().invoke("window:context-menu", options);
}

// ---- Dialogs ----

export type DialogOptions = {
	directory?: boolean;
	filters?: { name: string; extensions: string[] }[];
	title?: string;
	defaultPath?: string;
};

/** Shows a native open dialog on the host. */
export async function open(options: DialogOptions & { multiple: true }): Promise<string[] | null>;
/** Shows a native open dialog on the host. */
export async function open(options?: DialogOptions & { multiple?: false }): Promise<string | null>;
/**
 * Shows a native open dialog on the host.
 * @param options Dialog options; `multiple: true` widens the result to an array.
 * @returns The selected path(s), or `null` when cancelled.
 */
export async function open(
	options?: DialogOptions & { multiple?: boolean },
): Promise<string[] | string | null> {
	const paths = await eio().invoke("dialog:open", options ?? {});
	if (!paths) return null;
	return options?.multiple ? paths : (paths[0] ?? null);
}

export function save(options?: {
	defaultPath?: string;
	filters?: { name: string; extensions: string[] }[];
	title?: string;
}): Promise<string | null> {
	return eio().invoke("dialog:save", options ?? {});
}

// ---- Opener ----

export function openPath(p: string): Promise<void> {
	return eio().invoke("opener:open-path", { path: p });
}

export function openUrl(url: string): Promise<void> {
	return eio().invoke("opener:open-url", { url });
}

// ---- App ----

export function getVersion(): Promise<string> {
	return eio().invoke("app:version");
}

// ---- OS info (injected as non-writable constants by the main process) ----
export function platform(): string {
	return window.__os?.platform ?? "";
}

export function arch(): string {
	return window.__os?.arch ?? "";
}

export function type(): string {
	return window.__os?.type ?? "";
}

export function version(): string {
	return window.__os?.version ?? "";
}

// ---- HTTP ----

export const fetch = ((input: RequestInfo | URL, init?: RequestInit) =>
	window.fetch(input, init)) as typeof window.fetch;

// ---- Updater ----

export type DownloadEvent =
	| { event: "Started"; data: { contentLength: number } }
	| { event: "Progress"; data: { chunkLength: number } }
	| { event: "Finished"; data?: undefined };

class ElectronUpdate {
	constructor(
		private _version: string,
		private _currentVersion: string,
		private _notes: string | null = null,
	) {}

	get version(): string {
		return this._version;
	}

	get currentVersion(): string {
		return this._currentVersion;
	}

	/**
	 * Markdown body of the pending GitHub release, when the host could fetch it.
	 * @returns The release notes markdown, or null when unavailable.
	 */
	get notes(): string | null {
		return this._notes;
	}

	async downloadAndInstall(onEvent: (event: DownloadEvent) => void): Promise<void> {
		let started = false;
		const unsub = eio().on("updater:download-progress", (data) => {
			if (!started) {
				started = true;
				onEvent({ event: "Started", data: { contentLength: data.contentLength } });
			}
			onEvent({ event: "Progress", data: { chunkLength: data.chunkLength } });
		});

		try {
			await eio().invoke("updater:download");
			onEvent({ event: "Finished" });
		} finally {
			unsub();
		}
	}
}

export async function check(): Promise<ElectronUpdate | null> {
	const result = await eio().invoke("updater:check");
	if (result.error) throw new Error(result.error);
	if (!result.available || !result.version) return null;
	const currentVersion = await eio().invoke("app:version");
	return new ElectronUpdate(result.version, currentVersion, result.releaseNotes ?? null);
}

export type Update = ElectronUpdate;
