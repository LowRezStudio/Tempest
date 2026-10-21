import { createQuery, useQueryClient } from "@tanstack/svelte-query";
import { fetch, invoke } from "$lib/platform/api";
import { modRepositoryURL } from "$lib/stores/settings.svelte";

// Shapes from mods.lowrezstudio.com/contract/openapi.yaml (camelCase).
export type RepoSigner = {
	fingerprint: string;
	name: string;
	trust: "first-party" | "trusted" | "unknown" | "revoked";
};
export type RepoCompat = { patches: string[]; ranges: { from?: string; to?: string }[] };
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
	downloads: number;
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

export type BrowseParams = {
	q?: string;
	kind?: RepoMod["kind"] | "";
	patch?: string;
	sort?: "updated" | "downloads" | "created" | "name";
	page?: number;
};

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

export function browseRepo(params: BrowseParams): Promise<RepoPage> {
	const query = new URLSearchParams();
	if (params.q) query.set("q", params.q);
	if (params.kind) query.set("kind", params.kind);
	if (params.patch) query.set("patch", params.patch);
	if (params.sort) query.set("sort", params.sort);
	if (params.page) query.set("page", String(params.page));
	query.set("per_page", "24");
	return get<RepoPage>(`/api/v1/mods?${query}`);
}

export function getRepoMod(id: string): Promise<RepoMod & { readme: string }> {
	return get(`/api/v1/mods/${encodeURIComponent(id)}`);
}

export function getRepoVersions(id: string): Promise<RepoVersion[]> {
	return get(`/api/v1/mods/${encodeURIComponent(id)}/versions`);
}

// POSTs go through the main process: the repository drops bodies sent with the renderer's Origin.
function post(path: string, body: unknown): Promise<{ status: number; body: string }> {
	return invoke("repo:post-json", { url: `${repoBase()}${path}`, body });
}

// Newest compatible version for each installed mod; unknown ids are omitted by the server.
export async function checkRepoUpdates(
	patch: string,
	mods: { id: string; version: string }[],
): Promise<RepoUpdate[]> {
	if (!patch || mods.length === 0) return [];
	const res = await post("/api/v1/updates", { patch, mods: mods.slice(0, 200) });
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

export const createRepoBrowseQuery = (params: () => BrowseParams) =>
	createQuery(() => {
		const p = params();
		return {
			queryKey: ["repo", "browse", p],
			queryFn: () => browseRepo(p),
			staleTime: 60_000,
			retry: 1,
			placeholderData: (previous: RepoPage | undefined) => previous,
		};
	});

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

export const createRepoVersionsQuery = (id: () => string, enabled: () => boolean) =>
	createQuery(() => ({
		queryKey: ["repo", "versions", id()],
		queryFn: () => getRepoVersions(id()),
		enabled: enabled(),
		staleTime: 60_000,
	}));

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
