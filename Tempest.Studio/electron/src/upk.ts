// The `upk serve` backend: one long-lived child process speaking JSON lines
// over stdin/stdout (crates/tempest-assets/src/serve.rs). Requests are
// serialised by the server, so the client just numbers them and resolves
// the promise whose id comes back; lines without an id are events and are
// forwarded to every renderer.
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { app, BrowserWindow } from "electron";
import { handle } from "./ipc/handle.js";
import type { UpkEvent, UpkMethod, UpkStatus } from "./ipc/contract.js";

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

class UpkClient {
	private child: ChildProcessWithoutNullStreams | null = null;
	private pending = new Map<number, Pending>();
	private nextId = 1;
	private buffer = "";
	private lastError: string | null = null;
	private binary: string | null = null;

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
		this.buffer = "";
		child.stdout.setEncoding("utf8");
		child.stdout.on("data", (chunk: string) => this.onData(chunk));
		child.stderr.setEncoding("utf8");
		child.stderr.on("data", (chunk: string) => {
			for (const line of chunk.split("\n")) {
				if (line.trim()) console.log(`[upk] ${line}`);
			}
		});
		child.on("error", (error) => {
			this.lastError = error.message;
			this.failAll(error.message);
		});
		child.on("exit", (code, signal) => {
			if (this.child === child) this.child = null;
			this.failAll(`upk serve exited (code ${code}, signal ${signal})`);
			broadcast("upk:exit", { code, signal });
		});
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

	call(method: UpkMethod, params: unknown): Promise<unknown> {
		if (!this.child || this.child.exitCode !== null) this.start();
		const child = this.child;
		if (!child) return Promise.reject(new Error(this.lastError ?? "backend not running"));
		const id = this.nextId++;
		return new Promise((resolve, reject) => {
			this.pending.set(id, { resolve, reject, method });
			child.stdin.write(
				`${JSON.stringify({ id, method, params: params ?? {} })}\n`,
				(error) => {
					if (error) {
						this.pending.delete(id);
						reject(error);
					}
				},
			);
		});
	}

	private onData(chunk: string): void {
		this.buffer += chunk;
		let nl = this.buffer.indexOf("\n");
		while (nl >= 0) {
			const line = this.buffer.slice(0, nl);
			this.buffer = this.buffer.slice(nl + 1);
			if (line.trim()) this.onLine(line);
			nl = this.buffer.indexOf("\n");
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
			broadcast("upk:event", message as UpkEvent);
			return;
		}
		const id = message.id;
		if (typeof id !== "number") return;
		const pending = this.pending.get(id);
		if (!pending) return;
		this.pending.delete(id);
		if (message.ok) {
			pending.resolve(message.result ?? null);
		} else {
			pending.reject(new Error(String(message.error ?? "unknown error")));
		}
	}

	private failAll(reason: string): void {
		for (const [, pending] of this.pending) pending.reject(new Error(reason));
		this.pending.clear();
	}
}

function broadcast(channel: "upk:event" | "upk:exit", payload: unknown): void {
	for (const window of BrowserWindow.getAllWindows()) {
		if (!window.isDestroyed()) window.webContents.send(channel, payload);
	}
}

export const upk = new UpkClient();

handle("upk:call", (_event, { method, params }) => upk.call(method, params));
handle("upk:status", () => upk.status());
handle("upk:restart", () => upk.start());

app.on("will-quit", () => upk.stop());
