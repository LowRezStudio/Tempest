import { installDependenciesFor } from "$lib/mods/dependencies";
import { downloadModPackage } from "$lib/mods/download";
import { installModFile } from "$lib/mods/installFile";
import { trackModDownload } from "$lib/mods/modDownloads.svelte";
import { trackInstall } from "$lib/mods/pendingInstalls.svelte";
import { m } from "$lib/paraglide/messages";
import { getQueryClient } from "$lib/queries/client";
import { repoDownloadUrl } from "$lib/queries/repo";
import { addToast } from "$lib/stores/ui.svelte";
import type { DependencyMode } from "$lib/mods/dependencies";
import type { InstallOutcome } from "$lib/mods/installFile";
import type { ModDownloadTracker } from "$lib/mods/modDownloads.svelte";
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
		waitForGame?: boolean;
	} = {},
): Promise<InstallOutcome> {
	const tracker =
		options.tracker ?? trackInstall(gamePath, { name: displayName, version, modId }, true);
	let file: string;
	let record: ModDownloadTracker;
	try {
		const go = await installDependenciesFor(
			gamePath,
			modId,
			version,
			displayName,
			options.dependencies ?? "ask",
			options.waitForGame ?? false,
		);
		if (!go) {
			void tracker.finish(false);
			return { status: "cancelled" };
		}
		record = trackModDownload(gamePath, modId, displayName, version);
		file = await downloadModPackage(
			repoDownloadUrl(modId, version),
			`${modId}-${version}.tempest`,
			tracker,
			{ modId, version, sha256: options.sha256 },
			record,
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
		tracker: {
			...tracker,
			setPatching(progress) {
				tracker.setPatching(progress);
				record.setInstalling(progress * 100);
			},
		},
		dependencies: "skip",
		fromRepository: true,
		waitForGame: options.waitForGame,
	});
	// Waiting for the game to close, a queued install is still a finished download.
	record.finish(outcome.status === "installed" || outcome.status === "queued");
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
