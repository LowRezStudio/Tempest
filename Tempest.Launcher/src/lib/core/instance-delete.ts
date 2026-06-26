import { remove } from "@tauri-apps/plugin-fs";
import { restoreQueue } from "$lib/rigby/restore-queue";
import { removeInstance } from "$lib/stores/instance";
import type { Instance } from "$lib/types/instance";

export async function deleteInstance(instance: Instance, deleteData: boolean): Promise<void> {
	const isActive = instance.state.type === "downloading" || instance.state.type === "paused";

	if (isActive && instance.path) {
		restoreQueue.cancel(instance.path);
	}

	if (deleteData && instance.path) {
		try {
			await remove(instance.path, { recursive: true });
		} catch (error) {
			console.error("Failed to delete instance data:", error);
		}
	}

	removeInstance(instance.id);
}
