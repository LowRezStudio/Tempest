import { fetch } from "@tauri-apps/plugin-http";
import { persistedState } from "$lib/stores/persisted.svelte";

export type LatestRelease = {
	version: string;
	body: string;
	pub_date: string;
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
		};
		cachedReleaseNotes.value = release;
		return release;
	} catch {
		return null;
	}
}
