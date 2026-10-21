import { downloadRemoteMod } from "$lib/core/mods";
import type { InstallTracker } from "$lib/mods/pendingInstalls.svelte";

// Downloads a mod package, driving the placeholder row in the mod list when the install target
// is already known.
export async function downloadModPackage(
	url: string,
	filename: string,
	tracker?: InstallTracker,
): Promise<string> {
	tracker?.setProgress(0);
	const file = await downloadRemoteMod(
		url,
		filename,
		tracker && ((received, total) => tracker.setProgress(total ? received / total : null)),
	);
	tracker?.downloadDone();
	return file;
}
