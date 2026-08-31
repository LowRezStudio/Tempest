import { remove } from "@tauri-apps/plugin-fs";
import { listMods, removeMod } from "$lib/core/mods";
import { restoreQueue } from "$lib/rigby/restore-queue";
import { removeInstance } from "$lib/stores/instance.svelte";
import type { Instance } from "$lib/types/instance";

export type DeleteMode = "library" | "library_mods" | "delete";

/** @deprecated use DeleteMode */
export async function deleteInstance(
	instance: Instance,
	deleteDataOrMode: boolean | DeleteMode,
): Promise<void> {
	const mode: DeleteMode =
		typeof deleteDataOrMode === "boolean"
			? deleteDataOrMode
				? "delete"
				: "library"
			: deleteDataOrMode;

	const isActive = instance.state.type === "downloading" || instance.state.type === "paused";

	if (isActive && instance.path) {
		restoreQueue.cancel(instance.path);
	}

	if (mode === "library_mods" && instance.path) {
		try {
			const mods = await listMods(instance.path);
			for (const mod of mods) {
				try {
					await removeMod(instance.path, mod.Name);
				} catch (error) {
					console.error(`Failed to remove mod ${mod.Name}:`, error);
				}
			}
		} catch (error) {
			console.error("Failed to list mods for removal:", error);
		}
	}

	if (mode === "delete" && instance.path) {
		try {
			await remove(instance.path, { recursive: true });
		} catch (error) {
			console.error("Failed to delete instance data:", error);
		}
	}

	removeInstance(instance.id);
}
