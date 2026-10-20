import type { AuthorEntry, ConverterKind } from "$lib/mods/packaging";

type SourceEntry = {
	label: string;
	files: { path: string; data: Uint8Array }[];
	readme?: string;
};

/** The cooked package: a signed .tempest staged host-side, plus who signed it. */
type CookedPackage = {
	/** Host-side path of the signed package (temp dir); consumed by install/publish/save. */
	stagedPath: string;
	/** "user id [fingerprint]", as reported by `tempest mod sign`. */
	signer: string | null;
	/** Package size in bytes (the zip built before staging). */
	size: number;
};

class ConverterStore {
	sources = $state<SourceEntry[]>([]);
	modName = $state("");
	/** Repository id (slug). Empty means "derive from the name". */
	modId = $state("");
	modVersion = $state("1.0.0");
	summary = $state("");
	kind = $state<ConverterKind>("assets");
	tags = $state("");
	icon = $state<{ name: string; data: Uint8Array } | null>(null);
	authors = $state<AuthorEntry[]>([{ name: "", link: "" }]);
	readmeContent = $state("");
	cooked = $state<CookedPackage | null>(null);
}

export const converter = new ConverterStore();
