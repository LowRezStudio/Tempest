import { installMod } from "$lib/core/mods";
import { trackInstall } from "$lib/mods/pendingInstalls.svelte";
import { confirmReplaceMod, confirmUnverifiedMod } from "$lib/mods/ui.svelte";
import { m } from "$lib/paraglide/messages";
import { addToast } from "$lib/stores/ui.svelte";
import type { ModInstallResult } from "$lib/core/mods";
import type { InstallTracker } from "$lib/mods/pendingInstalls.svelte";

export type InstallOutcome =
	| { status: "installed"; result: ModInstallResult }
	| { status: "cancelled" }
	| { status: "failed"; message: string };

// The one install sequence: install, ask about an untrusted signer, ask about conflicts,
// install again accordingly. While it runs the mod shows as a placeholder row in the mod list
// (taking over `tracker` from a preceding download when given), filled in from the package's
// manifest; failures get an error toast, success toasts are left to the caller so batches can be
// summarised.
export async function installModFile(
	gamePath: string,
	filePath: string,
	displayName: string,
	options: { replace?: boolean; tracker?: InstallTracker } = {},
): Promise<InstallOutcome> {
	const tracker =
		options.tracker ??
		trackInstall(gamePath, { name: displayName.replace(/\.(tempest|zip|upk|pck)$/i, "") });
	await tracker.readPackage(filePath);
	const outcome = await runInstall(gamePath, filePath, displayName, options.replace ?? false);
	await tracker.finish(
		outcome.status === "installed",
		outcome.status === "installed" ? outcome.result.Mod?.Id : undefined,
	);
	return outcome;
}

async function runInstall(
	gamePath: string,
	filePath: string,
	displayName: string,
	replace: boolean,
): Promise<InstallOutcome> {
	try {
		let allowUntrusted = false;
		let res = await installMod(gamePath, filePath, replace, false);
		if (res.Unverified) {
			const confirmed = await confirmUnverifiedMod(
				displayName,
				res.Signer ?? "",
				!!res.KeyListOffline,
			);
			if (!confirmed) return { status: "cancelled" };
			allowUntrusted = true;
			res = await installMod(gamePath, filePath, replace, true);
		}
		if (res.Conflict) {
			const choice = await confirmReplaceMod(
				displayName,
				res.IsModConflict,
				res.ConflictingMods ?? [],
				res.NewModName ?? displayName,
			);
			if (choice === "stack") {
				res = await installMod(gamePath, filePath, false, allowUntrusted, true);
			} else if (choice === "replace") {
				res = await installMod(gamePath, filePath, true, allowUntrusted);
			} else return { status: "cancelled" };
		}
		if (res.Success) return { status: "installed", result: res };
		const message = res.Message || m.toast_installation_failed_unknown();
		addToast({
			title: m.toast_installation_failed_title(),
			message: `${displayName}: ${message}`,
			tone: "error",
		});
		return { status: "failed", message };
	} catch (error: unknown) {
		const message = (error as Error).message ?? m.toast_installation_failed_internal();
		addToast({
			title: m.toast_installation_failed_title(),
			message: `${displayName}: ${message}`,
			tone: "error",
		});
		return { status: "failed", message };
	}
}
