import { platform } from "@tauri-apps/plugin-os";
import { instanceMap } from "$lib/stores/instance.svelte";
import { launchingInstanceIds, processesList } from "$lib/stores/processes.svelte";

const gameRoot = (path: string) => {
	const root = path
		.replaceAll("\\", "/")
		.replace(/\/+$/, "")
		.replace(/\/Binaries(?:\/Win(?:32|64)(?:\/[^/]+\.exe)?)?$/i, "");
	return platform() === "windows" ? root.toLowerCase() : root;
};

/**
 * Prevent game-file changes while any session using that folder is starting or running.
 * @param path Game folder being modified.
 */
export function assertNoGameSessions(path: string): void {
	const root = gameRoot(path);
	const usesFolder = (other: string) => {
		const candidate = gameRoot(other);
		return (
			candidate === root ||
			candidate.startsWith(`${root}/`) ||
			root.startsWith(`${candidate}/`)
		);
	};
	if (
		processesList.value.some((process) => usesFolder(process.instance.path)) ||
		launchingInstanceIds.value.some((id) => {
			const instance = instanceMap.value[id];
			return instance && usesFolder(instance.path);
		})
	) {
		throw new Error("Close all sessions using this game folder before changing its files.");
	}
}
