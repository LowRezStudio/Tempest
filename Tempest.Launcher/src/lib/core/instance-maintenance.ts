import { m } from "$lib/paraglide/messages";
import { setupInstance } from "$lib/platforms/setup";
import { RIGBY_BASE_URL, RIGBY_MANIFEST_URL_TEMPLATE } from "$lib/rigby/constants";
import { restoreQueue } from "$lib/rigby/restore-queue";
import { updateInstance } from "$lib/stores/instance.svelte";
import { addToast } from "$lib/stores/ui.svelte";
import { patchOf } from "$lib/utils/versions";
import type { Instance } from "$lib/types/instance";

/**
 * The download manifest the instance's files are checked against.
 * @param instance - The instance.
 * @returns Its manifest id, or undefined when neither the instance nor its patch has one.
 */
export const manifestIdOf = (instance: Instance): string | undefined =>
	instance.manifestId ?? patchOf(instance.version)?.id;

/**
 * Runs setup again: the token and assembly export launching needs.
 * @param instance - The instance to set up.
 */
export async function runSetup(instance: Instance): Promise<void> {
	updateInstance(instance.id, { state: { type: "setup" } });
	try {
		await setupInstance(instance);
	} catch (error) {
		console.error("Instance setup failed:", error);
		addToast({
			title: m.instance_setup_failed({ name: instance.label }),
			message: error instanceof Error ? error.message : String(error),
			tone: "error",
		});
	} finally {
		updateInstance(instance.id, { state: { type: "prepared" } });
	}
}

/**
 * Queues a download that checks the instance's files against its patch and fetches what is
 * missing or damaged.
 * @param instance - The instance to verify.
 * @returns Whether it was queued; it needs a folder and a known manifest.
 */
export function verifyFiles(instance: Instance): boolean {
	const manifestId = manifestIdOf(instance);
	if (!instance.path || !manifestId) return false;
	restoreQueue.add({
		manifests: [RIGBY_MANIFEST_URL_TEMPLATE.replace("{id}", manifestId)],
		outDir: instance.path,
		baseUrl: RIGBY_BASE_URL,
		repair: true,
	});
	updateInstance(instance.id, { state: { type: "downloading" } });
	return true;
}
