import { downloadRemoteMod } from "$lib/core/mods";
import { installModFile } from "$lib/mods/installFile";
import { m } from "$lib/paraglide/messages";
import { getQueryClient } from "$lib/queries/client";
import { repoDownloadUrl } from "$lib/queries/repo";
import { addToast } from "$lib/stores/ui.svelte";
import type { InstallOutcome } from "$lib/mods/installFile";

// Downloads a version from the repository (counted) and runs the install sequence.
export async function installFromRepo(
	gamePath: string,
	modId: string,
	version: string,
	displayName = modId,
	options: { replace?: boolean } = {},
): Promise<InstallOutcome> {
	let file: string;
	try {
		file = await downloadRemoteMod(
			repoDownloadUrl(modId, version),
			`${modId}-${version}.tempest`,
		);
	} catch (error: unknown) {
		const message = (error as Error).message ?? "download failed";
		addToast({
			title: m.toast_installation_failed_title(),
			message: `${displayName}: ${message}`,
			tone: "error",
		});
		return { status: "failed", message };
	}
	const outcome = await installModFile(gamePath, file, `${displayName} ${version}`, options);
	if (outcome.status === "installed") {
		addToast({
			title: m.toast_mod_installed_title(),
			message: m.toast_mod_installed_message({
				name: outcome.result.Mod?.Name ?? displayName,
			}),
			tone: "success",
		});
		void getQueryClient()?.invalidateQueries({ queryKey: ["mods", gamePath] });
		void getQueryClient()?.invalidateQueries({ queryKey: ["repo", "updates"] });
	}
	return outcome;
}
