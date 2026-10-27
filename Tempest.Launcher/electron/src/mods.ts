// Mod package file operations: remote downloads, save/stage of cooked packs,
// "show in explorer" resolution and sandboxed mod file previews.
import { createHash, randomUUID } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { app, BrowserWindow, dialog, net, shell } from "electron";
import { handle } from "./ipc/handle.js";
import { parseManifest, readPackageManifest } from "./modPackage.js";
import type { ReadableStream as WebReadableStream } from "node:stream/web";

function toBuffer(data: Uint8Array): Buffer {
	return Buffer.from(data.buffer, data.byteOffset, data.byteLength);
}

// Writes a cooked pack to the temp dir so the CLI (or a save dialog) can read it.
export async function stagePack(filename: string, data: Uint8Array): Promise<string> {
	const stagedPath = path.join(os.tmpdir(), path.basename(filename));
	await fs.promises.writeFile(stagedPath, toBuffer(data));
	return stagedPath;
}

// Repository packages are kept per mod and version so switching back to a version skips the
// download. Only the newest few per mod stay.
const CACHED_VERSIONS_PER_MOD = 5;
const safeSegment = /^[0-9A-Za-z][0-9A-Za-z._+-]{0,63}$/;

function cacheDir(modId: string): string {
	if (!safeSegment.test(modId)) throw new Error(`invalid mod id: ${modId}`);
	return path.join(app.getPath("userData"), "mod-cache", modId);
}

async function sha256Of(file: string): Promise<string> {
	const hash = createHash("sha256");
	await pipeline(fs.createReadStream(file), hash);
	return hash.digest("hex");
}

async function pruneCache(dir: string): Promise<void> {
	const files = await fs.promises.readdir(dir).catch(() => []);
	const packages = await Promise.all(
		files
			.filter((f) => f.endsWith(".tempest"))
			.map(async (f) => ({ f, at: (await fs.promises.stat(path.join(dir, f))).mtimeMs })),
	);
	packages.sort((a, b) => b.at - a.at);
	for (const { f } of packages.slice(CACHED_VERSIONS_PER_MOD)) {
		await fs.promises.rm(path.join(dir, f), { force: true });
	}
}

handle("mods:cached-versions", async (_event, { modId }) => {
	const files = await fs.promises.readdir(cacheDir(modId)).catch(() => [] as string[]);
	return files.filter((f) => f.endsWith(".tempest")).map((f) => f.slice(0, -".tempest".length));
});

handle("mods:download-to-temp", async (event, { url, filename, progressId, cache }) => {
	let dest = path.join(os.tmpdir(), path.basename(filename));
	if (cache) {
		if (!safeSegment.test(cache.version)) throw new Error(`invalid version: ${cache.version}`);
		const dir = cacheDir(cache.modId);
		dest = path.join(dir, `${cache.version}.tempest`);
		if (fs.existsSync(dest)) {
			if (!cache.sha256 || (await sha256Of(dest)) === cache.sha256) {
				const now = new Date();
				await fs.promises.utimes(dest, now, now);
				return dest;
			}
			await fs.promises.rm(dest, { force: true });
		}
		await fs.promises.mkdir(dir, { recursive: true });
	}

	// Host-side download: no CORS/CSP involvement, and the bytes never enter
	// the renderer. Streamed to disk so big packages don't sit in memory; written next to the
	// target first so an interrupted download never looks like a complete cached package.
	const response = await net.fetch(url);
	if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
	const partial = `${dest}.part`;
	const total = Number(response.headers.get("content-length")) || null;
	let received = 0;
	let lastReport = 0;
	const report = () => {
		if (progressId && !event.sender.isDestroyed()) {
			event.sender.send("mods:download-progress", { id: progressId, received, total });
		}
	};
	const counter = new Transform({
		transform(chunk: Buffer, _encoding, done) {
			received += chunk.length;
			const now = Date.now();
			if (now - lastReport >= 100) {
				lastReport = now;
				report();
			}
			done(null, chunk);
		},
	});
	try {
		await pipeline(
			Readable.fromWeb(response.body as WebReadableStream<Uint8Array>),
			counter,
			fs.createWriteStream(partial),
		);
		if (cache?.sha256 && (await sha256Of(partial)) !== cache.sha256) {
			throw new Error("downloaded package does not match the repository's checksum");
		}
		await fs.promises.rename(partial, dest);
	} catch (error) {
		await fs.promises.rm(partial, { force: true });
		throw error;
	}
	report();
	if (cache) await pruneCache(path.dirname(dest));
	return dest;
});

handle("mods:read-manifest", (_event, { path: file }) => readPackageManifest(file));

handle("mods:stage-pack", (_event, { filename, data }) => stagePack(filename, data));

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

// Packages waiting in the launcher's install queue (an install refused because the game holds
// files it would replace), one folder per entry so the package keeps its own file name.
function installQueueDir(): string {
	return path.join(app.getPath("userData"), "install-queue");
}

// The entry folder of a copy `mods:queue-stage` made, or null for any other path.
function queueEntryOf(file: string): string | null {
	const entry = path.dirname(path.resolve(file));
	return path.dirname(entry) === path.resolve(installQueueDir()) ? entry : null;
}

handle("mods:queue-stage", async (_event, { path: file }) => {
	const entry = path.join(installQueueDir(), randomUUID());
	await fs.promises.mkdir(entry, { recursive: true });
	const dest = path.join(entry, path.basename(file));
	await fs.promises.copyFile(file, dest);
	return dest;
});

handle("mods:queue-discard", async (_event, { path: file }) => {
	const entry = queueEntryOf(file);
	if (entry) await fs.promises.rm(entry, { recursive: true, force: true }).catch(() => {});
});

handle("mods:queue-prune", async (_event, { keep }) => {
	const kept = new Set(keep.flatMap((file) => queueEntryOf(file) ?? []));
	const dir = installQueueDir();
	const entries = await fs.promises.readdir(dir).catch(() => [] as string[]);
	for (const name of entries) {
		const entry = path.join(dir, name);
		if (!kept.has(entry)) await fs.promises.rm(entry, { recursive: true, force: true });
	}
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

handle("mods:read-config", async (_event, { gamePath, modId }) => {
	if (!safeSegment.test(modId)) return null;
	const configFile = path.join(gamePath, "ChaosGame", "Config", "mods", `${modId}.toml`);
	const info = await fs.promises.stat(configFile).catch(() => null);
	if (!info?.isFile()) return null;
	return fs.promises.readFile(configFile, "utf8");
});

handle("mods:write-config", async (_event, { gamePath, modId, text }) => {
	if (!safeSegment.test(modId)) throw new Error("Invalid mod ID");
	if (Buffer.byteLength(text, "utf8") > 1024 * 1024) {
		throw new Error("Config files must be smaller than 1 MB");
	}
	const configFile = path.join(gamePath, "ChaosGame", "Config", "mods", `${modId}.toml`);
	const info = await fs.promises.lstat(configFile).catch(() => null);
	if (!info?.isFile()) throw new Error("This mod config file no longer exists");
	await fs.promises.writeFile(configFile, text, "utf8");
});

// The same formats and size cap the mod website accepts for icons.
const ICON_TYPES: Record<string, string> = {
	".png": "image/png",
	".webp": "image/webp",
	".jpg": "image/jpeg",
	".jpeg": "image/jpeg",
};
const ICON_LIMIT = 1024 * 1024;

handle("mods:installed-icon", async (_event, { gamePath, modId }) => {
	const modsDir = path.resolve(gamePath, ".tempest", "v2", "mods");
	const modDir = path.resolve(modsDir, modId);
	if (path.dirname(modDir) !== modsDir) return null;
	const manifest = await fs.promises
		.readFile(path.join(modDir, "manifest.toml"), "utf8")
		.catch(() => "");
	const file = path.resolve(modDir, parseManifest(manifest)?.icon ?? "icon.png");
	const type = ICON_TYPES[path.extname(file).toLowerCase()];
	if (!type || !file.startsWith(modDir + path.sep)) return null;
	const info = await fs.promises.stat(file).catch(() => null);
	if (!info?.isFile() || info.size > ICON_LIMIT) return null;
	return `data:${type};base64,${(await fs.promises.readFile(file)).toString("base64")}`;
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
