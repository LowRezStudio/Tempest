// Reads a .tempest package's manifest without unpacking it, so the launcher can show the mod's
// name, version and authors while the CLI is still installing it. Kept free of Electron imports.
import fs from "node:fs";
import zlib from "node:zlib";
import type { ModPackageInfo } from "./ipc/contract.js";

export async function readPackageManifest(file: string): Promise<ModPackageInfo | null> {
	if (!file.toLowerCase().endsWith(".tempest")) return null;
	try {
		const manifest = await readZipEntry(file, "manifest.toml");
		return manifest ? parseManifest(manifest.toString("utf8")) : null;
	} catch {
		return null;
	}
}

// Reads one entry of a zip through its central directory, without unpacking the rest.
// Zip64 archives (over 4 GB) are not handled and yield null.
async function readZipEntry(file: string, entryName: string): Promise<Buffer | null> {
	const fd = await fs.promises.open(file, "r");
	try {
		const read = async (position: number, length: number) => {
			const buffer = Buffer.alloc(length);
			const { bytesRead } = await fd.read(buffer, 0, length, position);
			return buffer.subarray(0, bytesRead);
		};
		const { size } = await fd.stat();
		const tailStart = Math.max(0, size - 0xffff - 22);
		const tail = await read(tailStart, size - tailStart);
		const eocd = tail.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
		if (eocd < 0) return null;
		const cdSize = tail.readUInt32LE(eocd + 12);
		const cdOffset = tail.readUInt32LE(eocd + 16);
		if (cdOffset === 0xffffffff) return null;
		const cd = await read(cdOffset, cdSize);
		for (let p = 0; p + 46 <= cd.length && cd.readUInt32LE(p) === 0x02014b50; ) {
			const method = cd.readUInt16LE(p + 10);
			const compressedSize = cd.readUInt32LE(p + 20);
			const nameLength = cd.readUInt16LE(p + 28);
			const extraLength = cd.readUInt16LE(p + 30);
			const commentLength = cd.readUInt16LE(p + 32);
			const localOffset = cd.readUInt32LE(p + 42);
			const name = cd.toString("utf8", p + 46, p + 46 + nameLength);
			p += 46 + nameLength + extraLength + commentLength;
			if (name !== entryName) continue;
			const local = await read(localOffset, 30);
			const dataStart = localOffset + 30 + local.readUInt16LE(26) + local.readUInt16LE(28);
			const data = await read(dataStart, compressedSize);
			if (method === 0) return data;
			if (method === 8) return zlib.inflateRawSync(data);
			return null;
		}
		return null;
	} finally {
		await fd.close();
	}
}

// Just the [mod] keys the launcher shows; the CLI does the real TOML parsing on install.
// Authors are either `authors = ["a", "b"]` or `[[mod.authors]]` tables with a `name`.
export function parseManifest(toml: string): ModPackageInfo | null {
	const info: ModPackageInfo = { id: "", name: "", version: "", authors: [] };
	const strings = (text: string) =>
		[...text.matchAll(/"((?:[^"\\]|\\.)*)"|'([^']*)'/g)].map((m) => m[1] ?? m[2] ?? "");
	let section = "";
	const lines = toml.split(/\r?\n/);
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i]!.trim();
		const header = /^\[\[?\s*([^\]]+?)\s*\]\]?$/.exec(line);
		if (header) {
			section = header[1]!;
			if (section === "mod.authors") info.authors.push("");
			continue;
		}
		const pair = /^([A-Za-z_]+)\s*=\s*(.*)$/.exec(line);
		if (!pair) continue;
		const [, key, rest] = pair;
		if (section === "mod.authors" && key === "name") {
			info.authors[info.authors.length - 1] = strings(rest!)[0] ?? "";
		} else if (section === "mod" && key === "authors") {
			let value = rest!;
			while (!value.includes("]") && i + 1 < lines.length) value += lines[++i];
			info.authors.push(...strings(value));
		} else if (
			section === "mod" &&
			(key === "id" || key === "name" || key === "version" || key === "icon")
		) {
			info[key] = strings(rest!)[0] ?? "";
		}
	}
	info.authors = info.authors.filter(Boolean);
	return info.id ? info : null;
}
