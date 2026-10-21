import { isCompatible } from "$lib/utils/version";
import type { RepoMod, RepoSort } from "$lib/queries/repo";

export type BrowseFilter = {
	q: string;
	kind: RepoMod["kind"] | null;
	tag: string | null;
	/** Only mods whose latest version runs on the instance's patch */
	compatibleOnly: boolean;
	hideInstalled: boolean;
	sort: RepoSort;
};

export const kinds: RepoMod["kind"][] = ["assets", "scripts", "native"];
export const sorts: RepoSort[] = ["downloads", "updated", "created", "name"];

const defaults = (): BrowseFilter => ({
	q: "",
	kind: null,
	tag: null,
	compatibleOnly: true,
	hideInstalled: false,
	sort: "downloads",
});

// Module state, so the filters survive switching tabs and instances.
export const browseFilter = $state<BrowseFilter>(defaults());

export function clearBrowseFilters() {
	const { sort } = browseFilter;
	Object.assign(browseFilter, { ...defaults(), sort });
}

export type BrowseContext = { patch: string | undefined; installed: Set<string> };

export const compatibleWith = (mod: RepoMod, patch: string | undefined) =>
	!!mod.latest && isCompatible(mod.latest.compat, patch);

export function matches(mod: RepoMod, f: BrowseFilter, ctx: BrowseContext): boolean {
	if (f.kind && mod.kind !== f.kind) return false;
	if (f.tag && !mod.tags.includes(f.tag)) return false;
	if (f.compatibleOnly && ctx.patch && !compatibleWith(mod, ctx.patch)) return false;
	if (f.hideInstalled && ctx.installed.has(mod.id)) return false;
	const q = f.q.trim().toLowerCase();
	if (q) {
		const hay = [mod.name, mod.summary, mod.id, mod.owner.displayName, ...mod.tags];
		if (!hay.some((h) => h.toLowerCase().includes(q))) return false;
	}
	return true;
}
