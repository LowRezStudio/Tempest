import { createQuery } from "@tanstack/svelte-query";
import { fetch } from "$lib/platform/api";
import { persistedState } from "$lib/stores/persisted.svelte";

export type LatestRelease = {
	version: string;
	body: string;
	pub_date: string;
	/** The release's GitHub page; missing from releases cached before it was stored. */
	url?: string;
};

export const cachedReleaseNotes = persistedState<LatestRelease | null>("homeReleaseNotes", null);

export async function fetchLatestRelease(): Promise<LatestRelease | null> {
	try {
		const controller = new AbortController();
		const timeout = setTimeout(() => controller.abort(), 5000);
		const res = await fetch(
			"https://api.github.com/repos/LowRezStudio/Tempest/releases/latest",
			{ signal: controller.signal },
		);
		clearTimeout(timeout);
		if (!res.ok) return null;
		const data = await res.json();
		const body = (data.body ?? "").trim();
		const release: LatestRelease = {
			version: data.tag_name?.replace(/^v/, "") ?? "",
			body,
			pub_date: data.published_at ?? "",
			url: data.html_url,
		};
		cachedReleaseNotes.value = release;
		return release;
	} catch {
		return null;
	}
}

export const createLatestReleaseQuery = () =>
	createQuery(() => ({
		queryKey: ["latest-release"],
		// Offline or rate limited: keep showing the last known release.
		queryFn: async () => (await fetchLatestRelease()) ?? cachedReleaseNotes.value,
		placeholderData: () => cachedReleaseNotes.value ?? undefined,
		staleTime: 15 * 60 * 1000,
		retry: 1,
	}));
