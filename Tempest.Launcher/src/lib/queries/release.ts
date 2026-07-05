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
			"https://github.com/LowRezStudio/Tempest/releases/latest/download/latest.json",
			{ signal: controller.signal },
		);
		clearTimeout(timeout);
		if (!res.ok) return null;
		const data = await res.json();
		const raw = (data.notes ?? "") as string;
		const body = raw.replace(/^chore: bump to v[^\n]*\n?/, "").trim();
		const release: LatestRelease = {
			version: data.version,
			body,
			pub_date: data.pub_date ?? "",
		};
		cachedReleaseNotes.value = release;
		return release;
	} catch {
		return null;
	}
}
