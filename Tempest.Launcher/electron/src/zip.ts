// Zip container I/O for the mod cooker: fflate owns the container in both
// directions, so no format bytes are read or written here.
import { unzipSync, zipSync } from "fflate";
import { handle } from "./ipc/handle.js";
import type { ZipEntry } from "./ipc/contract.js";

export function buildZip(entries: { path: string; data: Uint8Array | string }[]): Uint8Array {
	const encoder = new TextEncoder();
	const files: Record<string, Uint8Array> = {};
	for (const entry of entries) {
		files[entry.path] =
			typeof entry.data === "string" ? encoder.encode(entry.data) : entry.data;
	}
	return zipSync(files, { level: 9 });
}

// Archives end with directory entries the package layout has no use for.
export function readZip(bytes: Uint8Array): ZipEntry[] {
	return Object.entries(unzipSync(bytes))
		.filter(([path]) => !path.endsWith("/"))
		.map(([path, data]) => ({ path, data }));
}

handle("zip:build", (_event, { entries }) => buildZip(entries));
handle("zip:read", (_event, { data }) => readZip(data));
