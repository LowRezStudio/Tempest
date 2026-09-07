import fs from "node:fs";
import { ipcMain } from "electron";

const ERRNO_DESCRIPTIONS = {
	ENOENT: "No such file or directory",
	EACCES: "Permission denied",
	EPERM: "Operation not permitted",
	EEXIST: "File exists",
	ENOTDIR: "Not a directory",
	EISDIR: "Is a directory",
	EBUSY: "Device or resource busy",
	EMFILE: "Too many open files",
	ENAMETOOLONG: "File name too long",
	ELOOP: "Too many symbolic links encountered",
	ENOSPC: "No space left on device",
	EROFS: "Read-only file system",
	ENOTEMPTY: "Directory not empty",
};

function osErrorString(e) {
	if (e && e.code && ERRNO_DESCRIPTIONS[e.code] && typeof e.errno === "number") {
		return `${ERRNO_DESCRIPTIONS[e.code]} (os error ${-e.errno})`;
	}
	return e?.message ?? String(e);
}

function fail(prefix, p, e) {
	return { ok: false, error: `${prefix} ${p} with error: ${osErrorString(e)}` };
}

ipcMain.handle("fs:exists", (_event, { path: p }) => fs.existsSync(p));

ipcMain.handle("fs:read-dir", async (_event, { path: p }) => {
	try {
		const entries = await fs.promises.readdir(p, { withFileTypes: true });
		return {
			ok: true,
			data: entries.map((d) => ({
				name: d.name,
				isDirectory: d.isDirectory(),
				isFile: d.isFile(),
				isSymlink: d.isSymbolicLink(),
			})),
		};
	} catch (error) {
		return fail("failed to read directory at path:", p, error);
	}
});

ipcMain.handle("fs:stat", async (_event, { path: p }) => {
	try {
		const s = await fs.promises.stat(p);
		return {
			ok: true,
			data: {
				size: s.size,
				isDirectory: s.isDirectory(),
				isFile: s.isFile(),
				isSymlink: s.isSymlink(),
			},
		};
	} catch (error) {
		return fail("failed to get metadata of path:", p, error);
	}
});

ipcMain.handle("fs:remove", async (_event, { path: p, options }) => {
	try {
		await fs.promises.rm(p, options ?? {});
		return { ok: true, data: null };
	} catch (error) {
		return fail("failed to remove path:", p, error);
	}
});

ipcMain.handle("fs:mkdir", async (_event, { path: p, options }) => {
	try {
		await fs.promises.mkdir(p, { recursive: options?.recursive ?? false });
		return { ok: true, data: null };
	} catch (error) {
		return fail("failed to create directory at path:", p, error);
	}
});

ipcMain.handle("fs:read-file", async (_event, { path: p }) => {
	try {
		const data = await fs.promises.readFile(p);
		// Transfer bytes as an ArrayBuffer (~1x file size via structured clone).
		// Never spread into a number[]: a 30 MB file becomes a ~1 GB JS array
		// and OOM-crashes the app on real-world UPK/PCK sizes.
		const buf = data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
		return { ok: true, data: buf };
	} catch (error) {
		return fail("failed to read file at path:", p, error);
	}
});

ipcMain.handle("fs:write-file", async (_event, { path: p, data }) => {
	try {
		// Accept Uint8Array/ArrayBuffer (current renderer) and number[]
		// (older renderer builds) so mixed-version dev setups keep working.
		const bytes =
			data instanceof Uint8Array
				? data
				: data instanceof ArrayBuffer
					? new Uint8Array(data)
					: Uint8Array.from(data);
		await fs.promises.writeFile(
			p,
			Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength),
		);
		return { ok: true, data: null };
	} catch (error) {
		return fail("failed to write file at path:", p, error);
	}
});
