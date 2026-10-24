import { downloadRemoteMod } from "$lib/core/mods";
import type { InstallTracker } from "$lib/mods/pendingInstalls.svelte";

// Downloads a mod package, driving the placeholder row in the mod list when the install target
// is already known. With `cache`, a previously downloaded copy of that version is reused.
export async function downloadModPackage(
	url: string,
	filename: string,
	tracker?: InstallTracker,
	cache?: { modId: string; version: string; sha256?: string },
): Promise<string> {
	tracker?.setProgress(0);
	const file = await downloadRemoteMod(
		url,
		filename,
		tracker && ((received, total) => tracker.setProgress(total ? received / total : null)),
		cache,
	);
	tracker?.downloadDone();
	return file;
}
