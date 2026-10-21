import { downloadModPackage } from "$lib/mods/download";
import { installModFile } from "$lib/mods/installFile";
import { trackInstall } from "$lib/mods/pendingInstalls.svelte";
import { m } from "$lib/paraglide/messages";
import { getQueryClient } from "$lib/queries/client";
import { repoDownloadUrl } from "$lib/queries/repo";
import { addToast } from "$lib/stores/ui.svelte";
import type { InstallOutcome } from "$lib/mods/installFile";
import type { InstallTracker } from "$lib/mods/pendingInstalls.svelte";

// Downloads a version from the repository (counted; skipped when that version is already in the
// download cache, checked against `sha256` when given) and runs the install sequence, with the
// mod shown as a placeholder row in the list from the start (`tracker` when the caller already
// created it).
export async function installFromRepo(
	gamePath: string,
	modId: string,
	version: string,
	displayName = modId,
	options: { replace?: boolean; tracker?: InstallTracker; sha256?: string } = {},
): Promise<InstallOutcome> {
	const tracker =
		options.tracker ?? trackInstall(gamePath, { name: displayName, version, modId }, true);
	let file: string;
	try {
		file = await downloadModPackage(
			repoDownloadUrl(modId, version),
			`${modId}-${version}.tempest`,
			tracker,
			{ modId, version, sha256: options.sha256 },
		);
	} catch (error: unknown) {
		void tracker.finish(false);
		const message = (error as Error).message ?? "download failed";
		addToast({
			title: m.toast_installation_failed_title(),
			message: `${displayName}: ${message}`,
			tone: "error",
		});
		return { status: "failed", message };
	}
	const outcome = await installModFile(gamePath, file, `${displayName} ${version}`, {
		replace: options.replace,
		tracker,
	});
	void getQueryClient()?.invalidateQueries({ queryKey: ["mod-cache", modId] });
	if (outcome.status === "installed") {
		addToast({
			title: m.toast_mod_installed_title(),
			message: m.toast_mod_installed_message({
				name: outcome.result.Mod?.Name ?? displayName,
			}),
			tone: "success",
		});
		void getQueryClient()?.invalidateQueries({ queryKey: ["repo", "updates"] });
	}
	return outcome;
}
