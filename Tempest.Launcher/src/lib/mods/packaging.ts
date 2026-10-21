// Shared helpers for the mod cooker (single + batch): .tempest package parsing,
// repository-id slugs and .tempest package assembly.
import JSZip from "jszip";
import { invoke } from "$lib/platform/api";

/**
 * A file of a package. Most are game files (`path` is under the game folder, packed under
 * `files/`); `inModFolder` ones (DLLs, Lua) are packed at the package root and installed into
 * the mod's own folder.
 */
export type FileEntry = { path: string; data: Uint8Array; inModFolder?: boolean };
export type ConverterKind = "assets" | "scripts" | "native";
export type AuthorEntry = { name: string; link: string };
export type PackageIcon = { name: string; data: Uint8Array };

export type PackageManifest = {
	name: string;
	id: string;
	version: string;
	authors: AuthorEntry[];
	icon?: string;
	readme?: string;
};

// Regenerated when the package is signed again.
const SEAL_ENTRIES = new Set(["CHECKSUMS", "CHECKSUMS.asc", "SIGNER.pub"]);

// Same slug rule the repository and `tempest mod install <id>` enforce.
export const MOD_ID_PATTERN = /^[a-z0-9][a-z0-9._-]{1,63}$/;

export function targetDirFor(fileName: string): string {
	const ext = fileName.split(".").pop()?.toLowerCase() ?? "";
	if (ext === "ini") return "ChaosGame/Config";
	const name = fileName.toUpperCase();
	const isVoice = name.includes("_VOX") || name.includes("_VGS");
	return isVoice ? "ChaosGame/CookedPCConsole/English(US)" : "ChaosGame/CookedPCConsole";
}

// Win32 or Win64, from the PE header's machine field; null when it isn't a PE image.
export function peBits(data: Uint8Array): 32 | 64 | null {
	if (data.length < 0x40 || data[0] !== 0x4d || data[1] !== 0x5a) return null;
	const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
	const pe = view.getUint32(0x3c, true);
	if (pe + 6 > data.length || view.getUint32(pe, true) !== 0x00004550) return null;
	const machine = view.getUint16(pe + 4, true);
	return machine === 0x8664 ? 64 : machine === 0x014c ? 32 : null;
}

// dlls/64 and dlls/32 are injected only into the matching client; a DLL straight
// in dlls/ is offered to both, which is all that's left when its type is unknown.
function dllPath(fileName: string, data: Uint8Array): string {
	const bits = peBits(data);
	return bits ? `dlls/${bits}/${fileName}` : `dlls/${fileName}`;
}

// Where a loose file is cooked into: game files under the game folder, DLLs and
// Lua (scripts/, type stubs types/) into the mod's own folder.
export function looseFileEntry(fileName: string, data: Uint8Array): FileEntry {
	const lower = fileName.toLowerCase();
	if (lower.endsWith(".dll")) return { path: dllPath(fileName, data), data, inModFolder: true };
	if (lower.endsWith(".lua")) {
		const dir = lower.endsWith(".d.lua") ? "types" : "scripts";
		return { path: `${dir}/${fileName}`, data, inModFolder: true };
	}
	return { path: `${targetDirFor(fileName)}/${fileName}`, data };
}

// "Vexa Crosshair" -> "vexa-crosshair"; empty when nothing survives.
export function slugify(name: string): string {
	return name
		.toLowerCase()
		.replaceAll(/[^a-z0-9._-]+/g, "-")
		.replace(/^[-._]+/, "")
		.replace(/[-_]+$/, "")
		.slice(0, 64);
}

export function tomlEscape(value: string): string {
	return `"${value.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`;
}

export function parseManifest(text: string): PackageManifest {
	const result: PackageManifest = { name: "", id: "", version: "1.0.0", authors: [] };
	const lines = text.split("\n");
	let inAuthors = false;
	let currentAuthor: AuthorEntry | null = null;

	for (const raw of lines) {
		const line = raw.trim();
		if (!line || line.startsWith("#")) continue;
		if (line === "[[mod.authors]]") {
			if (currentAuthor) result.authors.push(currentAuthor);
			currentAuthor = { name: "", link: "" };
			inAuthors = true;
			continue;
		}
		const eqIdx = line.indexOf("=");
		if (eqIdx === -1) continue;
		const key = line.slice(0, eqIdx).trim();
		let val = line.slice(eqIdx + 1).trim();
		if (val.startsWith('"') && val.endsWith('"')) val = val.slice(1, -1);

		if (inAuthors) {
			if (key === "name" && currentAuthor) currentAuthor.name = val;
			else if (key === "link" && currentAuthor) currentAuthor.link = val;
		} else {
			if (key === "name") result.name = val;
			else if (key === "id") result.id = val;
			else if (key === "version") result.version = val;
			else if (key === "icon") result.icon = val;
			else if (key === "readme") result.readme = val;
		}
	}
	if (currentAuthor) result.authors.push(currentAuthor);
	return result;
}

// DLLs are native code, Lua is scripts, everything else is assets.
export function detectKind(files: FileEntry[]): ConverterKind {
	for (const file of files) {
		const path = file.path.toLowerCase();
		if (path.endsWith(".dll") || path.endsWith(".exe")) return "native";
	}
	for (const file of files) {
		const path = file.path.toLowerCase();
		if (path.endsWith(".lua") && !path.endsWith(".d.lua")) return "scripts";
	}
	return "assets";
}

export type ParsedPackage = {
	files: FileEntry[];
	manifest: PackageManifest | null;
	readme: string | null;
	icon: PackageIcon | null;
};

// Reads a .tempest package, old or already cooked: its game files, the files of
// its own mod folder, and the manifest, readme and icon. The signature files are
// dropped.
export async function parsePackage(data: Uint8Array): Promise<ParsedPackage> {
	const zip = await JSZip.loadAsync(data);
	const files: FileEntry[] = [];

	const manifestFile = zip.file("manifest.toml");
	const manifest = manifestFile ? parseManifest(await manifestFile.async("string")) : null;
	const readmeName = manifest?.readme ?? "README.md";
	const iconName = manifest?.icon ?? "icon.png";
	const readmeFile = zip.file(readmeName);
	const iconFile = zip.file(iconName);
	const readme = readmeFile ? await readmeFile.async("string") : null;
	const icon = iconFile
		? {
				name: iconName.split("/").pop() ?? iconName,
				data: new Uint8Array(await iconFile.async("arraybuffer")),
			}
		: null;
	const meta = new Set(["manifest.toml", "README.md", readmeName, iconName, ...SEAL_ENTRIES]);

	for (const [zipPath, zipEntry] of Object.entries(zip.files)) {
		if (zipEntry.dir) continue;
		const entryPath = zipPath.replaceAll("\\", "/");
		if (entryPath.startsWith("files/")) {
			let relativePath = entryPath.slice("files/".length);
			if (!relativePath) continue;
			// Old packages put game files straight into files/.
			if (!relativePath.includes("/")) {
				relativePath = `${targetDirFor(relativePath)}/${relativePath}`;
			}
			files.push({
				path: relativePath,
				data: new Uint8Array(await zipEntry.async("arraybuffer")),
			});
		} else if (!meta.has(entryPath)) {
			const bytes = new Uint8Array(await zipEntry.async("arraybuffer"));
			// Older packages put every DLL straight into dlls/; sort them by client.
			const legacyDll = /^dlls\/([^/]+\.dll)$/i.exec(entryPath)?.[1];
			files.push({
				path: legacyDll ? dllPath(legacyDll, bytes) : entryPath,
				data: bytes,
				inModFolder: true,
			});
		}
	}

	return { files, manifest, readme, icon };
}

export type PackageMeta = {
	id: string;
	name: string;
	version: string;
	authors: AuthorEntry[];
	readme?: string | null;
	icon?: PackageIcon | null;
};

export function buildManifest(meta: PackageMeta): string {
	const lines: string[] = [];
	lines.push("[mod]");
	lines.push(`id = ${tomlEscape(meta.id)}`);
	lines.push(`name = ${tomlEscape(meta.name)}`);
	lines.push(`version = ${tomlEscape(meta.version || "1.0.0")}`);
	if (meta.icon) lines.push(`icon = "icon.png"`);
	if (meta.readme?.trim()) lines.push(`readme = "README.md"`);
	lines.push("");
	for (const author of meta.authors) {
		if (!author.name.trim()) continue;
		lines.push("[[mod.authors]]");
		lines.push(`name = ${tomlEscape(author.name.trim())}`);
		if (author.link.trim()) lines.push(`link = ${tomlEscape(author.link.trim())}`);
		lines.push("");
	}
	return lines.join("\n");
}

// Compresses uncompressed .upk/.pck entries with the bundled upk-parser
// (host-side: bytes staged to a temp file), replacing them in the array.
// Entries the parser cannot handle are kept untouched.
export async function optimizeUpkEntries(
	files: FileEntry[],
	onProgress?: (name: string) => void,
): Promise<number> {
	let compressed = 0;
	for (const file of files) {
		const lower = file.path.toLowerCase();
		if (!lower.endsWith(".upk") && !lower.endsWith(".pck")) continue;
		onProgress?.(file.path.split("/").pop() ?? file.path);
		try {
			const result = await invoke("upk:optimize-data", {
				name: file.path.split("/").pop() ?? file.path,
				data: file.data,
			});
			if (result.optimized && result.data) {
				file.data = new Uint8Array(result.data);
				compressed++;
			}
		} catch (error) {
			console.warn("Could not optimize", file.path, error);
		}
	}
	return compressed;
}

// Builds the package zip: files/ and the mod-folder files, plus manifest.toml,
// README.md and icon.png. The package format forbids duplicate paths, so first
// occurrences win.
export async function buildPackageZip(
	meta: PackageMeta,
	files: FileEntry[],
): Promise<{ bytes: Uint8Array; duplicates: number }> {
	const zip = new JSZip();
	const seen = new Set<string>();
	let duplicates = 0;
	for (const file of files) {
		const entryPath = file.inModFolder ? file.path : `files/${file.path}`;
		if (seen.has(entryPath.toLowerCase())) {
			duplicates++;
			continue;
		}
		seen.add(entryPath.toLowerCase());
		zip.file(entryPath, file.data);
	}
	zip.file("manifest.toml", buildManifest(meta));
	if (meta.readme?.trim()) {
		zip.file("README.md", meta.readme.trim());
	}
	if (meta.icon) {
		zip.file("icon.png", meta.icon.data);
	}
	const bytes = await zip.generateAsync({ type: "uint8array", compression: "DEFLATE" });
	return { bytes, duplicates };
}
