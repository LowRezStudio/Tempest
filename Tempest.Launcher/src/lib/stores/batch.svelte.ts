import {
	MOD_ID_PATTERN,
	detectKind,
	optimizeUpkEntries,
	parsePackage,
	slugify,
	type ConverterKind,
	type FileEntry,
	type PackageIcon,
} from "$lib/mods/packaging";
import { m } from "$lib/paraglide/messages";

export type BatchStatus = "queued" | "cooking" | "signed" | "failed";

export type BatchEntry = {
	/** Unique id for list keys. */
	key: number;
	/** Original file name, e.g. "Vexa Crosshair.tempest". */
	label: string;
	name: string;
	id: string;
	version: string;
	authors: { name: string; link: string }[];
	readme: string;
	icon: PackageIcon | null;
	/** "id<requirement>" entries from the package's manifest, or the runtime for a Lua mod */
	dependencies: string[];
	kind: ConverterKind;
	tags: string;
	summary: string;
	files: FileEntry[];
	/** How many embedded package files were compressed on import. */
	compressedFiles: number;
	status: BatchStatus;
	/** Host-side path of the signed package, set once cooked. */
	stagedPath: string | null;
	signer: string | null;
	error: string | null;
};

let nextKey = 0;

async function entryFromImport(label: string, data: Uint8Array): Promise<BatchEntry> {
	const parsed = await parsePackage(data);
	if (parsed.files.length === 0) throw new Error(m.converter_no_files());
	// UPKs embedded in the uploaded mod get compressed if they are not
	// already (same treatment as the single-mod flow).
	const compressedFiles = await optimizeUpkEntries(parsed.files);
	const manifest = parsed.manifest;
	const manifestName = manifest?.name ?? "";
	const name = manifestName !== "" ? manifestName : label.replace(/\.tempest$/i, "");
	// An old manifest's id is often the display name (spaces, capitals); only
	// keep it when it already satisfies the repository's slug rule.
	const id = manifest?.id && MOD_ID_PATTERN.test(manifest.id) ? manifest.id : slugify(name);
	const manifestVersion = manifest?.version ?? "";
	return {
		key: ++nextKey,
		label,
		name,
		id,
		version: manifestVersion !== "" ? manifestVersion : "1.0.0",
		authors: manifest?.authors?.length
			? manifest.authors.map((a) => ({ name: a.name, link: a.link || "" }))
			: [],
		readme: parsed.readme ?? "",
		icon: parsed.icon,
		dependencies: manifest?.dependencies.length
			? manifest.dependencies
			: detectKind(parsed.files) === "scripts"
				? ["tempest-scripting"]
				: [],
		kind: detectKind(parsed.files),
		tags: "",
		summary: "",
		files: parsed.files,
		compressedFiles,
		status: "queued",
		stagedPath: null,
		signer: null,
		error: null,
	};
}

class BatchCookStore {
	entries = $state<BatchEntry[]>([]);

	// Throws when the package can't be read or has nothing to cook.
	async add(label: string, data: Uint8Array): Promise<number> {
		const entry = await entryFromImport(label, data);
		this.entries = [...this.entries, entry];
		return entry.compressedFiles;
	}

	remove(key: number) {
		this.entries = this.entries.filter((e) => e.key !== key);
	}

	clear() {
		this.entries = [];
	}
}

export const batch = new BatchCookStore();

/** Which cooker tab is active on the /converter route. */
export const cookMode = $state<{ value: "single" | "batch" }>({ value: "single" });
