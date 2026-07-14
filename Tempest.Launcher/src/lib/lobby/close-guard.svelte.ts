import { getCurrentWindow } from "@tauri-apps/api/window";
import { lobbyManager } from "$lib/lobby/lobby-manager";
import { appCloseLobbyWizardOpen } from "$lib/stores/ui.svelte";

$effect.root(() => {
	const appWindow = getCurrentWindow();
	let unlisten: (() => void) | undefined;
	void appWindow
		.onCloseRequested(async (event) => {
			event.preventDefault();
			if (lobbyManager.isConnected()) {
				appCloseLobbyWizardOpen.value = true;
			} else {
				await appWindow.destroy();
			}
		})
		.then((fn) => {
			unlisten = fn;
		});
	return () => unlisten?.();
});
