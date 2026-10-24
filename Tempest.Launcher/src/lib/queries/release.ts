import { createQuery } from "@tanstack/svelte-query";
import { fetch } from "$lib/platform/api";
import { persistedState } from "$lib/stores/persisted.svelte";

export type Release = {
	version: string;
	body: string;
	pub_date: string;
	/** The release's GitHub page */
	url?: string;
};

type GitHubRelease = {
	tag_name?: string;
	body?: string | null;
	published_at?: string | null;
	html_url?: string;
	draft?: boolean;
	prerelease?: boolean;
};

/** The launcher's releases as last fetched, newest first */
export const cachedReleases = persistedState<Release[]>("homeReleases", []);

export async function fetchReleases(): Promise<Release[] | null> {
	try {
		const controller = new AbortController();
		const timeout = setTimeout(() => controller.abort(), 5000);
		const res = await fetch(
			"https://api.github.com/repos/LowRezStudio/Tempest/releases?per_page=30",
			{ signal: controller.signal },
		);
		clearTimeout(timeout);
		if (!res.ok) return null;
		const data = (await res.json()) as GitHubRelease[];
		// The repository also releases Tempest Studio (studio-v*).
		const releases = data
			.filter((r) => !r.draft && !r.prerelease && /^v\d/.test(r.tag_name ?? ""))
			.map((r) => ({
				version: r.tag_name!.replace(/^v/, ""),
				body: (r.body ?? "").trim(),
				pub_date: r.published_at ?? "",
				url: r.html_url,
			}));
		cachedReleases.value = releases;
		return releases;
	} catch {
		return null;
	}
}

/**
 * @param enabled - Whether to fetch yet.
 * @returns The launcher's releases, newest first.
 */
export const createReleasesQuery = (enabled: () => boolean = () => true) =>
	createQuery(() => ({
		queryKey: ["releases"],
		// Offline or rate limited: keep showing the last known releases.
		queryFn: async () => (await fetchReleases()) ?? cachedReleases.value,
		placeholderData: () => (cachedReleases.value.length > 0 ? cachedReleases.value : undefined),
		enabled: enabled(),
		staleTime: 15 * 60 * 1000,
		retry: 1,
	}));
