import {
	MOD_ID_PATTERN,
	detectKind,
	type AuthorEntry,
	type ConverterKind,
	type FileEntry,
	type PackageIcon,
} from "$lib/mods/packaging";

/** What a .tempest package brought along besides its files. */
export type SourceMeta = {
	name: string;
	id: string;
	version: string;
	authors: AuthorEntry[];
	readme: string;
	icon: PackageIcon | null;
	/** "id<requirement>" entries of the package's manifest */
	dependencies: string[];
};

type SourceEntry = {
	label: string;
	files: FileEntry[];
	meta?: SourceMeta;
};

/** The cooked package: a signed .tempest staged host-side, plus who signed it. */
type CookedPackage = {
	/** Host-side path of the signed package (temp dir); consumed by install/publish/save. */
	stagedPath: string;
	/** "user id [fingerprint]", as reported by `tempest mod sign`. */
	signer: string | null;
	/** Package size in bytes (the zip built before staging). */
	size: number;
	/** The inputs it was cooked from, to tell when they have changed since. */
	from: string;
};

type Followed = "modName" | "modVersion" | "authors" | "readmeContent" | "icon" | "dependencies";

const empty = (): Pick<ConverterStore, Followed> => ({
	modName: "",
	modVersion: "1.0.0",
	authors: [{ name: "", link: "" }],
	readmeContent: "",
	icon: null,
	dependencies: "",
});

/** The dependency a Lua mod gets by default: the runtime that runs its scripts. */
export const SCRIPTING_RUNTIME = "tempest-scripting";

// Compares field values; package bytes compare by size, a full comparison is not worth it.
const same = (a: unknown, b: unknown) => {
	const key = (_: string, v: unknown) => (v instanceof Uint8Array ? v.length : v);
	return JSON.stringify(a, key) === JSON.stringify(b, key);
};

class ConverterStore {
	sources = $state<SourceEntry[]>([]);
	modName = $state("");
	/** Repository id (slug). Follows the package's id, else the name, until edited. */
	modId = $state("");
	modVersion = $state("1.0.0");
	summary = $state("");
	kind = $state<ConverterKind>("assets");
	tags = $state("");
	icon = $state<PackageIcon | null>(null);
	authors = $state<AuthorEntry[]>([{ name: "", link: "" }]);
	readmeContent = $state("");
	/** `[mod].dependencies`, one "id<requirement>" per line; Lua mods get the runtime by default. */
	dependencies = $state("");
	cooked = $state<CookedPackage | null>(null);
	/** The furthest step opened so far: 0 files, 1 details, 2 listing, 3 icon and readme. */
	reached = $state(0);
	/** The first valid repository id among the packages. */
	packageId = $state<string | null>(null);
	idTouched = $state(false);
	kindTouched = $state(false);
	/** What the packages last filled into each field, to tell it from what was typed. */
	private filled: Partial<Pick<ConverterStore, Followed>> = {};

	/**
	 * Fields a package filled in follow the packages: removing it takes back what it filled in,
	 * and the next package that has a value fills it in. Anything typed over stays.
	 */
	syncFromSources() {
		const metas = this.sources.flatMap((s) => (s.meta ? [s.meta] : []));
		const defaults = empty();
		this.follow("modName", metas.find((m) => m.name.trim())?.name ?? defaults.modName);
		this.follow(
			"modVersion",
			metas.find((m) => m.version.trim())?.version ?? defaults.modVersion,
		);
		this.follow(
			"authors",
			metas
				.find((m) => m.authors.length > 0)
				?.authors.map((a) => ({ name: a.name, link: a.link })) ?? defaults.authors,
		);
		this.follow(
			"readmeContent",
			metas.find((m) => m.readme.trim())?.readme ?? defaults.readmeContent,
		);
		this.follow("icon", metas.find((m) => m.icon)?.icon ?? defaults.icon);
		this.packageId = metas.find((m) => MOD_ID_PATTERN.test(m.id))?.id ?? null;
		if (!this.kindTouched) this.kind = detectKind(this.sources.flatMap((s) => s.files));
		// A package's own list, else the scripting runtime for a Lua mod (what every script mod
		// needs), else nothing.
		const fromPackage = metas.find((m) => m.dependencies.length > 0)?.dependencies;
		this.follow(
			"dependencies",
			fromPackage?.join("\n") ?? (this.kind === "scripts" ? SCRIPTING_RUNTIME : ""),
		);
	}

	// The dependency entries as typed, blanks dropped.
	get dependencyList(): string[] {
		return this.dependencies
			.split(/\r?\n|,/)
			.map((d) => d.trim())
			.filter(Boolean);
	}

	private follow<K extends Followed>(key: K, value: ConverterStore[K]) {
		const untouched = same(this[key], empty()[key]) || same(this[key], this.filled[key]);
		if (!untouched) return;
		(this as ConverterStore)[key] = value;
		this.filled[key] = value;
	}

	reset() {
		Object.assign(this, empty());
		this.sources = [];
		this.modId = "";
		this.summary = "";
		this.kind = "assets";
		this.tags = "";
		this.cooked = null;
		this.reached = 0;
		this.packageId = null;
		this.idTouched = false;
		this.kindTouched = false;
		this.filled = {};
	}
}

export const converter = new ConverterStore();
