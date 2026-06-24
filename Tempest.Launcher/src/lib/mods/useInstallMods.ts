import { useQueryClient } from "@tanstack/svelte-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { installMod } from "$lib/core/mods";
import { confirmReplaceMod, confirmUnverifiedMod } from "$lib/mods/ui";
import { m } from "$lib/paraglide/messages";
import { addToast, removeToast } from "$lib/stores/ui";

export function useInstallMods(instancePath: () => string | undefined) {
	const queryClient = useQueryClient();

	async function installMods() {
		const gamePath = instancePath();
		if (!gamePath) return;

		try {
			const result = await openDialog({
				directory: false,
				multiple: true,
				title: m.dialog_select_mod_files_title(),
				filters: [
					{
						name: m.dialog_select_mod_files_filter(),
						extensions: ["upk", "pck", "zip", "tempest"],
					},
				],
			});

			if (!result) return;

			const paths = Array.isArray(result) ? result : [result];
			let successCount = 0;
			let lastInstalledName = "";

			for (const filePath of paths) {
				const modFileName = filePath.split(/[/\\]/).pop() || filePath;
				let installingToastId: string | undefined;
				try {
					installingToastId = addToast({
						title: m.toast_mod_installing_title(),
						message: m.toast_mod_installing_message({ name: modFileName }),
						tone: "info",
						duration: 0,
					});

					let allowedUnsigned = false;
					let res = await installMod(gamePath, filePath, false, false);
					if (res.Unverified) {
						const confirmed = await confirmUnverifiedMod(modFileName);
						if (confirmed) {
							allowedUnsigned = true;
							res = await installMod(gamePath, filePath, false, true);
						} else {
							if (installingToastId) removeToast(installingToastId);
							continue;
						}
					}

					if (res.Conflict) {
						const confirmed = await confirmReplaceMod(modFileName, res.IsModConflict);
						if (confirmed) {
							res = await installMod(gamePath, filePath, true, allowedUnsigned);
						} else {
							if (installingToastId) removeToast(installingToastId);
							continue;
						}
					}

					if (installingToastId) removeToast(installingToastId);

					if (res.Success) {
						successCount++;
						lastInstalledName =
							res.Mod?.Name ?? modFileName ?? m.toast_mod_installed_fallback();
					} else {
						addToast({
							title: m.toast_installation_failed_title(),
							message: `${modFileName}: ${res.Message || m.toast_installation_failed_unknown()}`,
							tone: "error",
						});
					}
				} catch (error: any) {
					if (installingToastId) removeToast(installingToastId);
					addToast({
						title: m.toast_installation_failed_title(),
						message: `${modFileName}: ${error.message || m.toast_installation_failed_internal()}`,
						tone: "error",
					});
				}
			}

			if (successCount > 0) {
				if (successCount === 1) {
					addToast({
						title: m.toast_mod_installed_title(),
						message: m.toast_mod_installed_message({
							name: lastInstalledName,
						}),
						tone: "success",
					});
				} else {
					addToast({
						title: m.toast_mod_installed_title(),
						message: `Successfully installed ${successCount} mods`,
						tone: "success",
					});
				}
				queryClient.invalidateQueries({ queryKey: ["mods", gamePath] });
			}
		} catch (error: any) {
			addToast({
				title: m.toast_installation_failed_title(),
				message: error.message || m.toast_installation_failed_internal(),
				tone: "error",
			});
		}
	}

	return { installMods };
}
