// Shared helpers for the mod cooker (single + batch): V1 package parsing,
// repository-id slugs and .tempest package assembly.
import JSZip from "jszip";
import { invoke } from "$lib/platform/api";

export type FileEntry = { path: string; data: Uint8Array };
export type ConverterKind = "assets" | "scripts" | "native";
export type AuthorEntry = { name: string; link: string };

export type V1Manifest = {
	name: string;
	id: string;
	version: string;
	authors: AuthorEntry[];
};

// Same slug rule the repository and `tempest mod install <id>` enforce.
export const MOD_ID_PATTERN = /^[a-z0-9][a-z0-9._-]{1,63}$/;

export function targetDirFor(fileName: string): string {
	const ext = fileName.split(".").pop()?.toLowerCase() ?? "";
	if (ext === "ini") return "ChaosGame/Config";
	const name = fileName.toUpperCase();
	const isVoice = name.includes("_VOX") || name.includes("_VGS");
	return isVoice ? "ChaosGame/CookedPCConsole/English(US)" : "ChaosGame/CookedPCConsole";
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

export function parseV1Manifest(text: string): V1Manifest {
	const result: V1Manifest = { name: "", id: "", version: "1.0.0", authors: [] };
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

export type V1Package = {
	files: FileEntry[];
	manifest: V1Manifest | null;
	readme: string | null;
};

export async function parseV1Package(data: Uint8Array): Promise<V1Package> {
	const zip = await JSZip.loadAsync(data);
	const files: FileEntry[] = [];
	let manifest: V1Manifest | null = null;
	let readme: string | null = null;

	const manifestFile = zip.file("manifest.toml");
	if (manifestFile) {
		manifest = parseV1Manifest(await manifestFile.async("string"));
	}

	const readmeFile = zip.file("README.md");
	if (readmeFile) {
		readme = await readmeFile.async("string");
	}

	for (const [zipPath, zipEntry] of Object.entries(zip.files)) {
		if (!zipEntry.dir && zipPath.startsWith("files/")) {
			let relativePath = zipPath.slice("files/".length);
			if (relativePath) {
				if (!relativePath.includes("/")) {
					relativePath = `${targetDirFor(relativePath)}/${relativePath}`;
				}
				files.push({
					path: relativePath,
					data: new Uint8Array(await zipEntry.async("arraybuffer")),
				});
			}
		}
	}

	return { files, manifest, readme };
}

export type PackageMeta = {
	id: string;
	name: string;
	version: string;
	authors: AuthorEntry[];
	readme?: string | null;
	icon?: { name: string; data: Uint8Array } | null;
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

// Builds the package zip: files/ plus manifest.toml, README.md and icon.png.
// The package format forbids duplicate paths, so first occurrences win.
export async function buildPackageZip(
	meta: PackageMeta,
	files: FileEntry[],
): Promise<{ bytes: Uint8Array; duplicates: number }> {
	const zip = new JSZip();
	const seen = new Set<string>();
	let duplicates = 0;
	for (const file of files) {
		if (seen.has(file.path)) {
			duplicates++;
			continue;
		}
		seen.add(file.path);
		zip.file(`files/${file.path}`, file.data);
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
