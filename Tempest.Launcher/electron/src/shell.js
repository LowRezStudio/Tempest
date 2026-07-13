import { spawn, execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { ipcMain } from "electron";

const activeChildren = new Map();
let nextPid = 1;

ipcMain.handle("shell:spawn", (event, { program, args, options, sidecar }) => {
	let resolvedProgram = program;
	if (sidecar) {
		const ext = process.platform === "win32" ? ".exe" : "";
		resolvedProgram = path.join(process.resourcesPath, program + ext);
		if (!fs.existsSync(resolvedProgram)) {
			resolvedProgram = path.join(process.resourcesPath, "binaries", program + ext);
		}
	}

	const child = spawn(resolvedProgram, args ?? [], {
		...options,
		env: options?.env ? { ...process.env, ...options.env } : process.env,
		stdio: ["pipe", "pipe", "pipe"],
	});

	const id = nextPid++;
	activeChildren.set(id, child);

	child.stdout.on("data", (data) => {
		event.sender.send(`shell:process:${id}:stdout`, data.toString());
	});
	child.stderr.on("data", (data) => {
		event.sender.send(`shell:process:${id}:stderr`, data.toString());
	});
	child.on("close", (code, signal) => {
		event.sender.send(`shell:process:${id}:close`, {
			code: code ?? null,
			signal: signal ?? null,
		});
		activeChildren.delete(id);
	});
	child.on("error", (err) => {
		event.sender.send(`shell:process:${id}:error`, err.message);
		activeChildren.delete(id);
	});

	return { pid: id };
});

ipcMain.handle("shell:execute", (_event, { program, args, options }) => {
	return new Promise((resolve) => {
		execFile(
			program,
			args ?? [],
			{
				...options,
				env: options?.env ? { ...process.env, ...options.env } : process.env,
				maxBuffer: 100 * 1024 * 1024,
			},
			(err, stdout, stderr) => {
				resolve({ code: err?.code ?? 0, signal: err?.signal ?? null, stdout, stderr });
			},
		);
	});
});

ipcMain.handle("shell:kill", (_event, { pid }) => {
	const child = activeChildren.get(pid);
	if (child) {
		child.kill();
		activeChildren.delete(pid);
	}
});

ipcMain.handle("shell:stdin-write", (_event, { pid, data }) => {
	const child = activeChildren.get(pid);
	if (child?.stdin?.writable) child.stdin.write(data);
});

ipcMain.handle("which", (_event, { name }) => {
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

export { activeChildren };
