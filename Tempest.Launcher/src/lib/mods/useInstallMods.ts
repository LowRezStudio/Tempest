import { useQueryClient } from "@tanstack/svelte-query";
import { installModFiles } from "$lib/mods/installFile";
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
			if ((await installModFiles(gamePath, paths)) > 0) {
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
