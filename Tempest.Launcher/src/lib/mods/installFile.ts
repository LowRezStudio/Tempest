import { installMod } from "$lib/core/mods";
import { confirmReplaceMod, confirmUnverifiedMod } from "$lib/mods/ui.svelte";
import { m } from "$lib/paraglide/messages";
import { addToast, removeToast } from "$lib/stores/ui.svelte";
import type { ModInstallResult } from "$lib/core/mods";

export type InstallOutcome =
	| { status: "installed"; result: ModInstallResult }
	| { status: "cancelled" }
	| { status: "failed"; message: string };

// The one install sequence: install, ask about an untrusted signer, ask about conflicts,
// install again accordingly. Shows the "installing" toast while it runs and an error toast on
// failure; success toasts are left to the caller so batches can be summarised.
export async function installModFile(
	gamePath: string,
	filePath: string,
	displayName: string,
	options: { replace?: boolean } = {},
): Promise<InstallOutcome> {
	const toastId = addToast({
		title: m.toast_mod_installing_title(),
		message: m.toast_mod_installing_message({ name: displayName }),
		tone: "info",
		duration: 0,
	});
	try {
		let allowUntrusted = false;
		let res = await installMod(gamePath, filePath, options.replace ?? false, false);
		if (res.Unverified) {
			const confirmed = await confirmUnverifiedMod(
				displayName,
				res.Signer ?? "",
				!!res.KeyListOffline,
			);
			if (!confirmed) return { status: "cancelled" };
			allowUntrusted = true;
			res = await installMod(gamePath, filePath, options.replace ?? false, true);
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
	} finally {
		removeToast(toastId);
	}
}
