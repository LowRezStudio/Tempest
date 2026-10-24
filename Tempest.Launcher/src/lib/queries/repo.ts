import { createQuery, useQueryClient } from "@tanstack/svelte-query";
import { getAppVersion } from "$lib/core/appVersion";
import { fetch, invoke } from "$lib/platform/api";
import { modRepositoryURL } from "$lib/stores/settings.svelte";

// Shapes from mods.lowrezstudio.com/contract/openapi.yaml (camelCase).
export type RepoSigner = {
	fingerprint: string;
	name: string;
	trust: "first-party" | "trusted" | "unknown" | "revoked";
};
export type RepoCompat = { patches: string[]; ranges: { from?: string; to?: string }[] };
/** One `[mod].dependencies` entry of the package; the id `tempest` is the launcher itself. */
export type RepoDependency = { id: string; requirement: string };
export type RepoImage = {
	url: string;
	thumbUrl?: string;
	width: number;
	height: number;
	caption?: string;
};
export type RepoVersion = {
	id: string;
	modId: string;
	version: string;
	status: string;
	changelog: string;
	publishedAt: string | null;
	size: number;
	sha256: string;
	downloadUrl: string;
	signer: RepoSigner | null;
	code: { lua: number; dll: { win32: number; win64: number } };
	compat: RepoCompat;
	dependencies: RepoDependency[];
	downloads: number;
	assets?: { banner: RepoImage | null; screenshots: RepoImage[] };
};
export type RepoMod = {
	id: string;
	name: string;
	summary: string;
	kind: "assets" | "scripts" | "native";
	tags: string[];
	icon: string | null;
	owner: { handle: string; displayName: string; avatar: string | null; team: boolean };
	downloads: number;
	updatedAt: string;
	latest: RepoVersion | null;
	url: string;
};
export type RepoPage = { items: RepoMod[]; total: number; page: number; perPage: number };
export type RepoUpdate = {
	id: string;
	installed: string;
	latest: RepoVersion | null;
	updateAvailable: boolean;
	installedRevoked: boolean;
};

/** One mod of a resolved dependency tree (GET .../dependencies). */
export type RepoResolvedDependency = {
	id: string;
	name: string;
	requirements: string[];
	requiredBy: string[];
	resolved: RepoVersion | null;
	problem: "not_found" | "no_match" | null;
};
export type RepoResolution = {
	id: string;
	version: string;
	patch: string | null;
	tempest: { requirement: string; requiredBy: string }[];
	items: RepoResolvedDependency[];
};

export type RepoSort = "updated" | "downloads" | "created" | "name";
export type RepoTag = { tag: string; count: number };

export const repoBase = () => modRepositoryURL.value.replace(/\/$/, "");

async function get<T>(path: string, init?: RequestInit): Promise<T> {
	const controller = new AbortController();
	const timeout = setTimeout(() => controller.abort(), 15_000);
	try {
		const res = await fetch(`${repoBase()}${path}`, { ...init, signal: controller.signal });
		if (!res.ok) throw new Error(`${path}: HTTP ${res.status}`);
		return (await res.json()) as T;
	} finally {
		clearTimeout(timeout);
	}
}

// The whole listed catalog in the server's order, so browse filters run locally and instantly
// (like the website's browse page). Creation dates are not in the listing, hence the server sort.
export async function getRepoCatalog(sort: RepoSort): Promise<RepoMod[]> {
	const items: RepoMod[] = [];
	for (let page = 1; page <= 20; page++) {
		const res = await get<RepoPage>(`/api/v1/mods?sort=${sort}&page=${page}&per_page=60`);
		items.push(...res.items);
		if (res.items.length === 0 || items.length >= res.total) break;
	}
	return items;
}

export function getRepoTags(): Promise<RepoTag[]> {
	return get("/api/v1/tags");
}

export function getRepoMod(id: string): Promise<RepoMod & { readme: string }> {
	return get(`/api/v1/mods/${encodeURIComponent(id)}`);
}

export function getRepoVersions(id: string): Promise<RepoVersion[]> {
	return get(`/api/v1/mods/${encodeURIComponent(id)}/versions`);
}

// What a version needs, resolved by the repository to installable versions (dependencies of
// dependencies included) for the instance's patch and this launcher's version.
export function getRepoDependencies(
	id: string,
	version: string,
	patch: string | undefined,
	tempest: string,
): Promise<RepoResolution> {
	const query = new URLSearchParams({ tempest });
	if (patch) query.set("patch", patch);
	return get(
		`/api/v1/mods/${encodeURIComponent(id)}/versions/${encodeURIComponent(version)}/dependencies?${query}`,
	);
}

// POSTs go through the main process: the repository drops bodies sent with the renderer's Origin.
function post(path: string, body: unknown): Promise<{ status: number; body: string }> {
	return invoke("repo:post-json", { url: `${repoBase()}${path}`, body });
}

// Newest compatible version for each installed mod (one this launcher can run: the server
// skips versions whose `tempest` requirement is newer); unknown ids are omitted by the server.
export async function checkRepoUpdates(
	patch: string,
	mods: { id: string; version: string }[],
): Promise<RepoUpdate[]> {
	if (!patch || mods.length === 0) return [];
	const tempest = await getAppVersion().catch(() => undefined);
	const res = await post("/api/v1/updates", { patch, tempest, mods: mods.slice(0, 200) });
	if (res.status !== 200) throw new Error(`/api/v1/updates: HTTP ${res.status}`);
	return JSON.parse(res.body) as RepoUpdate[];
}

export type RepoBugReport = { threadUrl: string | null };

// Anonymous "get help" report on a mod: the server opens (or appends to) a Discord
// help thread. Error responses carry the server's `{error, message}`; its message is
// what the user sees (e.g. the one-per-mod-per-day limit).
export async function reportRepoBug(
	id: string,
	input: { body: string; version?: string; gameVersion?: string },
): Promise<RepoBugReport> {
	const res = await post(`/api/v1/mods/${encodeURIComponent(id)}/bugs`, {
		...input,
		source: "launcher",
	});
	const parsed = (() => {
		try {
			return JSON.parse(res.body) as unknown;
		} catch {
			return null;
		}
	})();
	if (res.status < 200 || res.status >= 300) {
		throw new Error((parsed as { message?: string } | null)?.message ?? `HTTP ${res.status}`);
	}
	return parsed as RepoBugReport;
}

// The URL the launcher downloads from: counted by the repository, redirects to the CDN.
export function repoDownloadUrl(modId: string, version: string): string {
	return `${repoBase()}/api/v1/mods/${encodeURIComponent(modId)}/versions/${encodeURIComponent(version)}/download`;
}

export const createRepoCatalogQuery = (sort: () => RepoSort) =>
	createQuery(() => {
		const s = sort();
		return {
			queryKey: ["repo", "catalog", s],
			queryFn: () => getRepoCatalog(s),
			staleTime: 60_000,
			retry: 1,
			placeholderData: (previous: RepoMod[] | undefined) => previous,
		};
	});

export const createRepoTagsQuery = () =>
	createQuery(() => ({
		queryKey: ["repo", "tags"],
		queryFn: getRepoTags,
		staleTime: 10 * 60_000,
		retry: 1,
	}));

// Repository entries of installed mods, by id. Each one goes through the per-mod cache entry
// RepoModModal uses, so opening a mod's page does not fetch it again.
export const createRepoModsQuery = (ids: () => string[]) => {
	const queryClient = useQueryClient();
	return createQuery(() => {
		const sorted = [...ids()].sort();
		return {
			queryKey: ["repo", "mods", sorted],
			queryFn: async () => {
				const mods = await Promise.all(
					sorted.map((id) =>
						queryClient
							.fetchQuery({
								queryKey: ["repo", "mod", id],
								queryFn: () => getRepoMod(id),
								staleTime: 10 * 60_000,
							})
							.catch(() => null),
					),
				);
				return new Map(
					mods.flatMap((mod) => (mod ? [[mod.id, mod as RepoMod] as const] : [])),
				);
			},
			enabled: sorted.length > 0,
			staleTime: 10 * 60_000,
		};
	});
};

// How many approved versions each installed mod has on the repository, by id. Goes through the
// per-mod versions entry RepoModModal reads, so its Versions tab opens without a fetch.
export const createRepoVersionCountsQuery = (ids: () => string[]) => {
	const queryClient = useQueryClient();
	return createQuery(() => {
		const sorted = [...ids()].sort();
		return {
			queryKey: ["repo", "version-counts", sorted],
			queryFn: async () => {
				const counts = await Promise.all(
					sorted.map((id) =>
						queryClient
							.fetchQuery({
								queryKey: ["repo", "versions", id],
								queryFn: () => getRepoVersions(id),
								staleTime: 60_000,
							})
							.then(
								(versions) =>
									versions.filter((v) => v.status === "approved").length,
							)
							.catch(() => 0),
					),
				);
				return new Map(sorted.map((id, i) => [id, counts[i]!]));
			},
			enabled: sorted.length > 0,
			staleTime: 60_000,
		};
	});
};

export const createRepoUpdatesQuery = (
	input: () => { patch: string; mods: { id: string; version: string }[] },
) =>
	createQuery(() => {
		const { patch, mods } = input();
		return {
			queryKey: ["repo", "updates", patch, mods.map((m) => `${m.id}@${m.version}`).sort()],
			queryFn: () => checkRepoUpdates(patch, mods),
			enabled: !!patch && mods.length > 0,
			staleTime: 10 * 60_000,
			retry: 1,
		};
	});
