// Mod package file operations: remote downloads, save/stage of cooked packs,
// "show in explorer" resolution and sandboxed mod file previews.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { BrowserWindow, dialog, net, shell } from "electron";
import { handle } from "./ipc/handle.js";

function toBuffer(data: Uint8Array): Buffer {
	return Buffer.from(data.buffer, data.byteOffset, data.byteLength);
}

handle("mods:download-to-temp", async (_event, { url, filename }) => {
	// Host-side download: no CORS/CSP involvement, and the bytes never enter
	// the renderer.
	const response = await net.fetch(url);
	if (!response.ok) throw new Error(`HTTP ${response.status}`);
	const dest = path.join(os.tmpdir(), path.basename(filename));
	await fs.promises.writeFile(dest, Buffer.from(await response.arrayBuffer()));
	return dest;
});

handle("mods:save-pack", async (event, { defaultPath, data }) => {
	// Save dialog + write in one round trip: the renderer only has the bytes.
	const win = BrowserWindow.fromWebContents(event.sender);
	const result = await dialog.showSaveDialog(win!, {
		defaultPath,
		filters: [{ name: "Tempest Mod", extensions: ["tempest"] }],
	});
	if (result.canceled || !result.filePath) return null;
	await fs.promises.writeFile(result.filePath, toBuffer(data));
	return result.filePath;
});

handle("mods:stage-pack", async (_event, { filename, data }) => {
	// Write a cooked pack next to the CLI's temp workspace so `mod install`
	// can consume it; the caller discards it after installing.
	const stagedPath = path.join(os.tmpdir(), path.basename(filename));
	await fs.promises.writeFile(stagedPath, toBuffer(data));
	return stagedPath;
});

handle("mods:save-staged-pack", async (event, { stagedPath }) => {
	// Save dialog + copy of an already-cooked package in the temp dir (e.g. a
	// signed one the CLI wrote): the bytes never cross the bridge.
	const win = BrowserWindow.fromWebContents(event.sender);
	const result = await dialog.showSaveDialog(win!, {
		defaultPath: path.basename(stagedPath),
		filters: [{ name: "Tempest Mod", extensions: ["tempest"] }],
	});
	if (result.canceled || !result.filePath) return null;
	await fs.promises.copyFile(stagedPath, result.filePath);
	return result.filePath;
});

handle("mods:save-staged-batch", async (event, { files }) => {
	// One folder pick, then every cooked package is copied in as <filename>.
	const win = BrowserWindow.fromWebContents(event.sender);
	const result = await dialog.showOpenDialog(win!, {
		properties: ["openDirectory", "createDirectory"],
		title: "Choose a folder for the cooked mods",
	});
	const dir = result.filePaths[0];
	if (result.canceled || !dir) return null;
	const saved: string[] = [];
	for (const file of files) {
		const dest = path.join(dir, path.basename(file.filename));
		await fs.promises.copyFile(file.stagedPath, dest);
		saved.push(dest);
	}
	return { dir, saved };
});

handle("mods:discard-staged-pack", async (_event, { stagedPath }) => {
	// Only ever delete inside the OS temp dir.
	const resolved = path.resolve(stagedPath);
	const tmp = path.resolve(os.tmpdir());
	if (!resolved.startsWith(tmp + path.sep)) return;
	await fs.promises.rm(resolved, { force: true }).catch(() => {});
});

handle("mods:open-location", async (_event, { gamePath, mod }) => {
	let folder: string;
	if (mod.kind === "V2") {
		folder = path.join(gamePath, ".tempest", "v2", "mods", mod.modId);
	} else {
		const target = mod.targetPath;
		folder = target;
		try {
			const info = await fs.promises.stat(target);
			if (info.isFile()) folder = path.dirname(target);
		} catch {
			// Missing file: if it looks like a file, open its presumed folder.
			if (target.includes(".") && !target.endsWith("/") && !target.endsWith("\\")) {
				folder = path.dirname(target);
			}
		}
	}
	const error = await shell.openPath(folder);
	if (error) throw new Error(error);
});

handle("mods:has-dlls", async (_event, { gamePath, modId }) => {
	try {
		await fs.promises.access(path.join(gamePath, ".tempest", "v2", "mods", modId, "dlls"));
		return true;
	} catch {
		return false;
	}
});

const PREVIEW_LIMIT = 50_000;

handle("mods:read-file", async (_event, { basePath, modId, path: target }) => {
	const sep = basePath.includes("\\") ? "\\" : "/";
	let resolved: string;
	if (target.toLowerCase().endsWith(".ini") && modId) {
		// For .ini, show the mod fragment that is to be merged (stored in
		// .tempest/v2/mods/<modId>/files/...), not the final merged game ini.
		const normalizedBase = `${basePath.replaceAll("\\", "/").toLowerCase()}/`;
		const relative = target.replaceAll("\\", "/").toLowerCase().startsWith(normalizedBase)
			? target.slice(basePath.length).replace(/^[\\/]+/, "")
			: target.replace(/^[\\/]+/, "");
		const osRelative = relative.replaceAll("/", sep).replaceAll("\\", sep);
		resolved = `${basePath.replace(/[\\/]+$/, "")}${sep}.tempest${sep}v2${sep}mods${sep}${modId}${sep}files${sep}${osRelative}`;
	} else {
		// Resolve relative paths against basePath (InstalledFiles can be relative)
		const isAbsolute =
			/^[a-zA-Z]:[\\/]/.test(target) || target.startsWith("/") || target.startsWith("\\");
		resolved = isAbsolute
			? target
			: `${basePath.replace(/[\\/]+$/, "")}${sep}${target.replace(/^[\\/]+/, "")}`;
	}

	// Containment guard: previews may only read inside the instance directory.
	const normalized = path.resolve(resolved);
	const baseResolved = path.resolve(basePath);
	const same = (p: string) => (process.platform === "win32" ? p.toLowerCase() : p);
	if (!same(normalized).startsWith(`${same(baseResolved)}${path.sep}`)) {
		throw new Error(`refusing to read ${normalized}: outside the instance directory`);
	}

	const file = await fs.promises.open(normalized, "r");
	try {
		const buffer = Buffer.alloc(PREVIEW_LIMIT + 1);
		const { bytesRead } = await file.read(buffer, 0, buffer.length, 0);
		return {
			text: buffer.toString("utf8", 0, bytesRead),
			truncated: bytesRead > PREVIEW_LIMIT,
		};
	} finally {
		await file.close();
	}
});
