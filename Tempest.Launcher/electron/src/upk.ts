// UPK optimization for the mod cooker via the bundled `upk` tool (the Rust
// package/asset CLI, which also applies mod deltas): reports whether a package
// is already stored LZO-compressed, and compresses uncompressed ones (cooker
// chunking, obscured when the source was) so player mods install like game
// assets.
import { execFile } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { app } from "electron";
import { handle } from "./ipc/handle.js";

const NAME = process.platform === "win32" ? "upk.exe" : "upk";

/**
 * The upk binary: $TEMPEST_UPK, the Rust release build in a dev run
 * (getAppPath() is Tempest.Launcher/electron, the workspace is two up), or the
 * bundled sidecar.
 * @returns Absolute path to the binary, or null.
 */
function upkBinary(): string | null {
	const env = process.env.TEMPEST_UPK;
	if (env && fs.existsSync(env)) return env;
	const dev = path.resolve(
		app.getAppPath(),
		"..",
		"..",
		"Tempest.UnrealScript",
		"target",
		"release",
		NAME,
	);
	if (!app.isPackaged && fs.existsSync(dev)) return dev;
	const bundled = path.join(process.resourcesPath, "binaries", NAME);
	return fs.existsSync(bundled) ? bundled : null;
}

function run(
	bin: string,
	args: string[],
	timeout = 120_000,
): Promise<{ code: number; stdout: string; stderr: string }> {
	return new Promise((resolve, reject) => {
		execFile(
			bin,
			args,
			{ timeout, windowsHide: true, maxBuffer: 1024 * 1024 },
			(err, stdout, stderr) => {
				if (err && typeof err.code !== "number") return reject(err);
				resolve({
					code: err ? (typeof err.code === "number" ? err.code : 1) : 0,
					stdout: stdout?.toString() ?? "",
					stderr: stderr?.toString() ?? "",
				});
			},
		);
	});
}

const REPORT_TIMEOUT = 120_000;
const COMPRESS_TIMEOUT = 10 * 60_000;

/**
 * Reports + compresses a package file on disk.
 * @param bin Path of the upk binary.
 * @param file Path of the package file on disk.
 * @returns Whether the file was already compressed, and the compressed bytes
 * (null when nothing changed).
 */
async function optimizeOnDisk(
	bin: string,
	file: string,
): Promise<{ optimized: boolean; data: ArrayBuffer | null }> {
	const info = await run(bin, ["is-compressed", file], REPORT_TIMEOUT);
	if (info.code !== 0) {
		throw new Error(info.stderr.trim() || `upk exited ${info.code}`);
	}
	if (info.stdout.trim() === "true") return { optimized: false, data: null };

	const out = path.join(os.tmpdir(), `tempest-upk-${Date.now()}-${path.basename(file)}`);
	try {
		const saved = await run(bin, ["compress", file, out], COMPRESS_TIMEOUT);
		if (saved.code !== 0) {
			throw new Error(saved.stderr.trim() || `upk exited ${saved.code}`);
		}
		const data = await fs.promises.readFile(out);
		return {
			optimized: true,
			data: data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength),
		};
	} finally {
		await fs.promises.rm(out, { force: true }).catch(() => {});
	}
}

handle("upk:optimize", (_event, { path: file }) => {
	const bin = upkBinary();
	if (!bin) throw new Error("upk is not available");
	return optimizeOnDisk(bin, file);
});

// Same, for a package that only exists as bytes (extracted from a .tempest
// upload): staged to a temp file, optimized, cleaned up.
handle("upk:optimize-data", async (_event, { name, data }) => {
	const bin = upkBinary();
	if (!bin) throw new Error("upk is not available");
	const inPath = path.join(os.tmpdir(), `tempest-upk-in-${Date.now()}-${path.basename(name)}`);
	await fs.promises.writeFile(inPath, Buffer.from(data.buffer, data.byteOffset, data.byteLength));
	try {
		return await optimizeOnDisk(bin, inPath);
	} finally {
		await fs.promises.rm(inPath, { force: true }).catch(() => {});
	}
});
