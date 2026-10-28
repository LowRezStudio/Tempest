// The `upk serve` backend: one long-lived child process per window speaking
// JSON lines over stdin/stdout (crates/tempest-assets/src/serve.rs). A
// server holds one workspace, so every window owns its own server and can
// work on another game folder. Requests are serialised by the server, so the
// client just numbers them and resolves the promise whose id comes back;
// lines without an id are events and are forwarded to the owning window.
//
// Framed responses: after the `protocol` handshake the server answers a
// request with a small header line `{"id", "ok", "ms", "len", "bin": [..]}`
// followed by `len` bytes of result JSON and then the binary payloads whose
// lengths `bin` lists. The host forwards the JSON bytes and the payloads to
// the renderer untouched, which parses the JSON once (no base64, no
// structured clone of a 65k-row table) and drops each payload where the
// result holds a `{"$bin": n}` marker (`src/lib/studio/rpc.ts`).
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { app, type WebContents } from "electron";
import { handle } from "./ipc/handle.js";
import type { UpkEvent, UpkFrame, UpkMethod, UpkStatus } from "./ipc/contract.js";

const EXE = process.platform === "win32" ? "upk.exe" : "upk";

/** Where the backend binary lives: bundled sidecar, or the Rust release build in dev. */
export function upkBinary(): string | null {
	const env = process.env.TEMPEST_UPK;
	if (env && fs.existsSync(env)) return env;
	const bundled = path.join(process.resourcesPath, "binaries", EXE);
	if (app.isPackaged && fs.existsSync(bundled)) return bundled;
	// Dev: app path is Tempest.Studio/electron, the Rust workspace is a sibling of Tempest.Studio.
	const dev = path.resolve(
		app.getAppPath(),
		"..",
		"..",
		"Tempest.UnrealScript",
		"target",
		"release",
		EXE,
	);
	if (fs.existsSync(dev)) return dev;
	return fs.existsSync(bundled) ? bundled : null;
}

type Pending = {
	resolve: (value: unknown) => void;
	reject: (error: Error) => void;
	method: string;
};

/** A header line whose frames are still being read. */
type Reading = { header: Record<string, unknown>; need: number };

export class UpkClient {
	private child: ChildProcessWithoutNullStreams | null = null;
	private pending = new Map<number, Pending>();
	private nextId = 1;
	private buffer: Buffer = Buffer.alloc(0);
	private reading: Reading | null = null;
	private lastError: string | null = null;
	private binary: string | null = null;

	/** `owner` receives the server's events; null for a client without a window. */
	constructor(private readonly owner: WebContents | null) {}

	status(): UpkStatus {
		return {
			running: this.child !== null && this.child.exitCode === null,
			binary: this.binary,
			pid: this.child?.pid ?? null,
			error: this.lastError,
		};
	}

	start(): UpkStatus {
		this.stop();
		this.binary = upkBinary();
		if (!this.binary) {
			this.lastError =
				"upk binary not found (build Tempest.UnrealScript with `cargo build --release`)";
			return this.status();
		}
		this.lastError = null;
		const child = spawn(this.binary, ["serve"], {
			stdio: ["pipe", "pipe", "pipe"],
			windowsHide: true,
			env: { ...process.env, RUST_BACKTRACE: "1" },
		});
		this.child = child;
		this.buffer = Buffer.alloc(0);
		this.reading = null;
		child.stdout.on("data", (chunk: Buffer) => this.onData(chunk));
		child.stderr.setEncoding("utf8");
		child.stderr.on("data", (chunk: string) => {
			for (const line of chunk.split("\n")) {
				if (line.trim()) console.log(`[upk ${child.pid}] ${line}`);
			}
		});
		child.on("error", (error) => {
			this.lastError = error.message;
			this.failAll(error.message);
		});
		child.on("exit", (code, signal) => {
			if (this.child === child) this.child = null;
			this.failAll(`upk serve exited (code ${code}, signal ${signal})`);
			this.emit("upk:exit", { code, signal });
		});
		// Switch to framed responses before anything else is sent.
		this.write(0, "protocol", { frames: true });
		return this.status();
	}

	stop(): void {
		const child = this.child;
		this.child = null;
		if (child && child.exitCode === null) {
			try {
				child.stdin.write(`${JSON.stringify({ id: 0, method: "quit" })}\n`);
			} catch {
				// ignore
			}
			setTimeout(() => {
				if (child.exitCode === null) child.kill();
			}, 500);
		}
		this.failAll("backend stopped");
	}

	/** Stops the server for good once its window is gone. Nobody is left to receive the
	 * replies, so pending calls are dropped instead of rejected: a rejection would only be
	 * logged by Electron as an error in the `upk:call` handler. */
	dispose(): void {
		this.pending.clear();
		this.stop();
	}

	private write(id: number, method: string, params: unknown, onError?: (e: Error) => void): void {
		this.child?.stdin.write(
			`${JSON.stringify({ id, method, params: params ?? {} })}\n`,
			(error) => {
				if (error && onError) onError(error);
			},
		);
	}

	call(method: UpkMethod, params: unknown): Promise<unknown> {
		if (!this.child || this.child.exitCode !== null) this.start();
		const child = this.child;
		if (!child) return Promise.reject(new Error(this.lastError ?? "backend not running"));
		const id = this.nextId++;
		return new Promise((resolve, reject) => {
			this.pending.set(id, { resolve, reject, method });
			this.write(id, method, params, (error) => {
				this.pending.delete(id);
				reject(error);
			});
		});
	}

	private onData(chunk: Buffer): void {
		this.buffer = this.buffer.length === 0 ? chunk : Buffer.concat([this.buffer, chunk]);
		for (;;) {
			if (this.reading) {
				if (this.buffer.length < this.reading.need) return;
				const { header, need } = this.reading;
				this.reading = null;
				const bytes = this.buffer.subarray(0, need);
				this.buffer = this.buffer.subarray(need);
				this.onFrames(header, bytes);
				continue;
			}
			const nl = this.buffer.indexOf(10);
			if (nl < 0) return;
			const line = this.buffer.subarray(0, nl).toString("utf8");
			this.buffer = this.buffer.subarray(nl + 1);
			if (line.trim()) this.onLine(line);
		}
	}

	private onLine(line: string): void {
		let message: Record<string, unknown>;
		try {
			message = JSON.parse(line) as Record<string, unknown>;
		} catch {
			console.warn(`[upk] unparsable line: ${line.slice(0, 200)}`);
			return;
		}
		if (typeof message.event === "string") {
			this.emit("upk:event", message as UpkEvent);
			return;
		}
		if (typeof message.len === "number") {
			const bins = Array.isArray(message.bin) ? (message.bin as number[]) : [];
			const need = message.len + bins.reduce((a, b) => a + b, 0);
			this.reading = { header: message, need };
			return;
		}
		this.settle(message, null);
	}

	/** The result JSON and payloads that followed a header line. */
	private onFrames(header: Record<string, unknown>, bytes: Buffer): void {
		const len = header.len as number;
		const lengths = Array.isArray(header.bin) ? (header.bin as number[]) : [];
		// Copies: the shared read buffer must not travel to the renderer.
		const json = new Uint8Array(bytes.subarray(0, len));
		const bins: Uint8Array[] = [];
		let at = len;
		for (const n of lengths) {
			bins.push(new Uint8Array(bytes.subarray(at, at + n)));
			at += n;
		}
		this.settle(header, { json, bins });
	}

	private settle(message: Record<string, unknown>, frame: UpkFrame | null): void {
		const id = message.id;
		if (typeof id !== "number") return;
		const pending = this.pending.get(id);
		if (!pending) return;
		this.pending.delete(id);
		if (message.ok) {
			pending.resolve(frame ?? { json: null, value: message.result ?? null, bins: [] });
		} else {
			pending.reject(new Error(String(message.error ?? "unknown error")));
		}
	}

	private failAll(reason: string): void {
		for (const [, pending] of this.pending) pending.reject(new Error(reason));
		this.pending.clear();
	}

	private emit(channel: "upk:event" | "upk:exit", payload: unknown): void {
		const target = this.owner;
		if (target && !target.isDestroyed()) target.send(channel, payload);
	}
}

/** One backend per window, keyed by the window's WebContents id. */
const clients = new Map<number, UpkClient>();

/** The backend of a window, spawned on first use and stopped when the window goes. */
export function upkFor(sender: WebContents): UpkClient {
	let client = clients.get(sender.id);
	if (!client) {
		const created = new UpkClient(sender);
		clients.set(sender.id, created);
		sender.once("destroyed", () => {
			clients.delete(sender.id);
			created.dispose();
		});
		client = created;
	}
	return client;
}

export function stopAllUpk(): void {
	for (const client of clients.values()) client.dispose();
	clients.clear();
}

handle("upk:call", (event, { method, params }) => upkFor(event.sender).call(method, params));
handle("upk:status", (event) => upkFor(event.sender).status());
handle("upk:restart", (event) => upkFor(event.sender).start());

app.on("will-quit", () => stopAllUpk());
