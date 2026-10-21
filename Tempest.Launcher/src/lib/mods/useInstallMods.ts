import { useQueryClient } from "@tanstack/svelte-query";
import { installModFile } from "$lib/mods/installFile";
import { m } from "$lib/paraglide/messages";
import { open as openDialog } from "$lib/platform/api";
import { addToast } from "$lib/stores/ui.svelte";

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
				const modFileName = filePath.split(/[/\\]/).pop() ?? filePath;
				const outcome = await installModFile(gamePath, filePath, modFileName);
				if (outcome.status === "installed") {
					successCount++;
					lastInstalledName = outcome.result.Mod?.Name ?? modFileName;
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
				void queryClient.invalidateQueries({ queryKey: ["mods", gamePath] });
			}
		} catch (error: any) {
			addToast({
				title: m.toast_installation_failed_title(),
				message: error.message ?? m.toast_installation_failed_internal(),
				tone: "error",
			});
		}
	}

	return { installMods };
}
