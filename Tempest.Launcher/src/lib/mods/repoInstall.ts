import { installDependenciesFor } from "$lib/mods/dependencies";
import { downloadModPackage } from "$lib/mods/download";
import { installModFile } from "$lib/mods/installFile";
import { trackInstall } from "$lib/mods/pendingInstalls.svelte";
import { m } from "$lib/paraglide/messages";
import { getQueryClient } from "$lib/queries/client";
import { repoDownloadUrl } from "$lib/queries/repo";
import { addToast } from "$lib/stores/ui.svelte";
import type { DependencyMode } from "$lib/mods/dependencies";
import type { InstallOutcome } from "$lib/mods/installFile";
import type { InstallTracker } from "$lib/mods/pendingInstalls.svelte";

// Installs what the version depends on first (asking, unless `dependencies` says otherwise),
// then downloads it from the repository (counted; skipped when that version is already in the
// download cache, checked against `sha256` when given) and runs the install sequence, with the
// mod shown as a placeholder row in the list from the start (`tracker` when the caller already
// created it).
export async function installFromRepo(
	gamePath: string,
	modId: string,
	version: string,
	displayName = modId,
	options: {
		replace?: boolean;
		tracker?: InstallTracker;
		sha256?: string;
		dependencies?: DependencyMode;
	} = {},
): Promise<InstallOutcome> {
	const tracker =
		options.tracker ?? trackInstall(gamePath, { name: displayName, version, modId }, true);
	let file: string;
	try {
		const go = await installDependenciesFor(
			gamePath,
			modId,
			version,
			displayName,
			options.dependencies ?? "ask",
		);
		if (!go) {
			void tracker.finish(false);
			return { status: "cancelled" };
		}
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
		dependencies: "skip",
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
