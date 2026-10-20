import { createQuery } from "@tanstack/svelte-query";
import { fetch } from "$lib/platform/api";
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
	owner: { handle: string; displayName: string; team: boolean };
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

// Newest compatible version for each installed mod; unknown ids are omitted by the server.
export function checkRepoUpdates(
	patch: string,
	mods: { id: string; version: string }[],
): Promise<RepoUpdate[]> {
	if (!patch || mods.length === 0) return Promise.resolve([]);
	return get<RepoUpdate[]>("/api/v1/updates", {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify({ patch, mods: mods.slice(0, 200) }),
	});
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
