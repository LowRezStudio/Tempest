// UPK optimization for the mod cooker via the bundled `upk-parser` zig tool:
// reports whether a package is already compressed, and compresses uncompressed
// ones (LZO, cooker-style chunks) so player mods install like game assets.
import { execFile } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { app } from "electron";
import { handle } from "./ipc/handle.js";

const NAME = process.platform === "win32" ? "upk-parser.exe" : "upk-parser";

/**
 * The bundled upk-parser, or null (dev run without a local build).
 * @returns Absolute path to the binary, or null.
 */
function upkParserBinary(): string | null {
	const bundled = path.join(process.resourcesPath, "binaries", NAME);
	if (fs.existsSync(bundled)) return bundled;
	// Dev run: the parser lives in the repo's zig build output. In dev,
	// getAppPath() is Tempest.Launcher/electron, so the repo root is two up.
	const dev = path.resolve(
		app.getAppPath(),
		"..",
		"..",
		"Tempest.Utils",
		"utils",
		"zig-out",
		"bin",
		NAME,
	);
	return fs.existsSync(dev) ? dev : null;
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
 * @param bin Path of the upk-parser binary.
 * @param file Path of the package file on disk.
 * @returns Whether the file was already compressed, and the compressed bytes
 * (null when nothing changed).
 */
async function optimizeOnDisk(
	bin: string,
	file: string,
): Promise<{ optimized: boolean; data: ArrayBuffer | null }> {
	const res = await run(bin, [file, "-report"], REPORT_TIMEOUT);
	if (res.code !== 0) {
		throw new Error(res.stderr.trim() || `upk-parser exited ${res.code}`);
	}
	// "Compression:  none" | "Compression:  stored compressed" | "fully compressed"
	const compression =
		/Compression:\s*(\S[^\n]*)/m.exec(res.stdout)?.[1]?.trim().toLowerCase() ?? "";
	const compressed = compression !== "" && !compression.startsWith("none");
	if (compressed) return { optimized: false, data: null };

	const out = path.join(os.tmpdir(), `tempest-upk-${Date.now()}-${path.basename(file)}`);
	try {
		const saved = await run(bin, [file, "-save", "compressed", out], COMPRESS_TIMEOUT);
		if (saved.code !== 0) {
			throw new Error(saved.stderr.trim() || `upk-parser exited ${saved.code}`);
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
	const bin = upkParserBinary();
	if (!bin) throw new Error("upk-parser is not available");
	return optimizeOnDisk(bin, file);
});

// Same, for a package that only exists as bytes (extracted from a .tempest
// upload): staged to a temp file, optimized, cleaned up.
handle("upk:optimize-data", async (_event, { name, data }) => {
	const bin = upkParserBinary();
	if (!bin) throw new Error("upk-parser is not available");
	const inPath = path.join(os.tmpdir(), `tempest-upk-in-${Date.now()}-${path.basename(name)}`);
	await fs.promises.writeFile(inPath, Buffer.from(data.buffer, data.byteOffset, data.byteLength));
	try {
		return await optimizeOnDisk(bin, inPath);
	} finally {
		await fs.promises.rm(inPath, { force: true }).catch(() => {});
	}
});
