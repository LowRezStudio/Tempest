import { execFile, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { handle } from "./ipc/handle.js";

const activeChildren = new Map<number, ReturnType<typeof spawn>>();
let nextPid = 1;

// The sender's webContents can be destroyed while a child is still running
// (window closed, app quitting); sending to it would throw in the main process.
const send = (sender: Electron.WebContents, channel: string, payload: unknown) => {
	if (!sender.isDestroyed()) sender.send(channel, payload);
};

const detached = process.platform !== "win32";

function killChild(child: ReturnType<typeof spawn>) {
	if (detached && child.pid !== undefined) {
		try {
			process.kill(-child.pid, "SIGTERM");
			return;
		} catch {
			/* group already gone; fall back to the child itself */
		}
	}
	child.kill();
}

function resolveSidecar(program: string): string {
	const ext = process.platform === "win32" ? ".exe" : "";
	const direct = path.join(process.resourcesPath, program + ext);
	if (fs.existsSync(direct)) return direct;
	return path.join(process.resourcesPath, "binaries", program + ext);
}

handle("shell:spawn", (event, { program, args, options, sidecar }) => {
	const resolvedProgram = sidecar ? resolveSidecar(program) : program;

	const child = spawn(resolvedProgram, args, {
		...options,
		env: options?.env ? { ...process.env, ...options.env } : process.env,
		stdio: ["pipe", "pipe", "pipe"],
		detached,
	});

	const id = nextPid++;
	activeChildren.set(id, child);

	let closed = false;
	const notifyClose = (code: number | null, signal: NodeJS.Signals | null) => {
		if (closed) return;
		closed = true;
		send(event.sender, "shell:process", {
			pid: id,
			type: "close",
			code: code ?? null,
			signal: signal ?? null,
		});
		activeChildren.delete(id);
	};

	child.stdout?.on("data", (data: Buffer) => {
		send(event.sender, "shell:process", { pid: id, type: "stdout", data: data.toString() });
	});
	child.stderr?.on("data", (data: Buffer) => {
		send(event.sender, "shell:process", { pid: id, type: "stderr", data: data.toString() });
	});
	// The child's own termination ('exit') is the reliable "launch is over" signal.
	// 'close' waits for the stdio pipes to close, and on Proton/Wine the wrapper's
	// grandchildren inherit those fds and can hold them open long after the game is
	// gone, so 'close' may never fire and the game stays marked as running forever.
	child.on("exit", (code, signal) => notifyClose(code, signal));
	child.on("close", (code, signal) => notifyClose(code, signal));
	child.on("error", (err) => {
		send(event.sender, "shell:process", { pid: id, type: "error", message: err.message });
		activeChildren.delete(id);
	});

	return { pid: id };
});

handle("shell:execute", (_event, { program, args, options, sidecar }) => {
	const resolvedProgram = sidecar ? resolveSidecar(program) : program;
	return new Promise((resolve) => {
		execFile(
			resolvedProgram,
			args,
			{
				...options,
				env: options?.env ? { ...process.env, ...options.env } : process.env,
				maxBuffer: 100 * 1024 * 1024,
			},
			(err, stdout, stderr) => {
				// A signal-killed child has err.code === null; a spawn failure has a
				// string code like "ENOENT". Only a clean exit is success (0).
				const code = err ? (typeof err.code === "number" ? err.code : null) : 0;
				resolve({ code, signal: err?.signal ?? null, stdout, stderr });
			},
		);
	});
});

handle("shell:kill", (_event, { pid }) => {
	const child = activeChildren.get(pid);
	if (child) {
		killChild(child);
		activeChildren.delete(pid);
	}
});

handle("shell:stdin-write", (_event, { pid, data }) => {
	const child = activeChildren.get(pid);
	if (child?.stdin?.writable) child.stdin.write(data);
});

handle("shell:which", (_event, { name }) => {
	const ext = process.platform === "win32" ? ".exe" : "";
	const dirs = (process.env.PATH ?? "").split(path.delimiter);
	for (const dir of dirs) {
		const candidate = path.join(dir, name + ext);
		try {
			if (fs.existsSync(candidate)) {
				try {
					fs.accessSync(candidate, fs.constants.X_OK);
				} catch {
					continue;
				}
				return candidate;
			}
		} catch {
			continue;
		}
	}
	return null;
});

export { activeChildren, killChild };
