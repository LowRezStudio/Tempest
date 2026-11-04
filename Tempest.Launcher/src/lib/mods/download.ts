import { downloadRemoteMod } from "$lib/core/mods";
import type { ModDownloadTracker } from "$lib/mods/modDownloads.svelte";
import type { InstallTracker } from "$lib/mods/pendingInstalls.svelte";

// Downloads a mod package, driving the placeholder row in the mod list when the install target
// is already known and the Downloads page's entry (`record`). With `cache`, a previously
// downloaded copy of that version is reused.
export async function downloadModPackage(
	url: string,
	filename: string,
	tracker?: InstallTracker,
	cache?: { modId: string; version: string; sha256?: string },
	record?: ModDownloadTracker,
): Promise<string> {
	tracker?.setProgress(0);
	const file = await downloadRemoteMod(
		url,
		filename,
		(received, total) => {
			tracker?.setProgress(total ? received / total : null);
			record?.progress(received, total);
		},
		cache,
	).catch((error: unknown) => {
		record?.finish(false);
		throw error;
	});
	record?.setInstalling();
	tracker?.downloadDone();
	return file;
}
