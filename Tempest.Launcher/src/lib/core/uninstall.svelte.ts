import { path } from "@tauri-apps/api";
import { appConfigDir } from "@tauri-apps/api/path";
import { exists, mkdir, readDir, readFile, rename, writeFile } from "@tauri-apps/plugin-fs";
import { createCommand } from "$lib/core/command";
import { restoreQueue } from "$lib/rigby/restore-queue";
import { instanceMap, instanceOrder, lastLaunchedInstanceId } from "$lib/stores/instance.svelte";
import { addToast } from "$lib/stores/ui.svelte";
import type { Instance } from "$lib/types/instance";

type Registry = {
	version: 1;
	cleaned: boolean;
	removedIds?: string[];
	instances: Pick<Instance, "id" | "label" | "path" | "managedPath" | "origin">[];
};
let writes = Promise.resolve();
let cleaning = false;
export const cleanupRegistryState = $state({ ready: false });

export async function downloadOwnership(
	target: string,
): Promise<Pick<Instance, "origin" | "managedPath">> {
	if (!(await exists(target)) || (await readDir(target)).length === 0) {
		return { origin: "download", managedPath: target };
	}
	return { origin: "import" };
}

export async function saveCleanupRegistry(instances: Instance[]): Promise<string> {
	const config = await appConfigDir();
	const target = await path.join(config, "uninstall-instances.json");
	const snapshot: Registry = {
		version: 1,
		cleaned: false,
		instances: instances.map(({ id, label, path: gamePath, managedPath, origin }) => ({
			id,
			label,
			path: gamePath,
			managedPath,
			origin,
		})),
	};
	const write = writes.then(async () => {
		await mkdir(config, { recursive: true });
		const temporary = `${target}.tmp`;
		await writeFile(temporary, new TextEncoder().encode(JSON.stringify(snapshot)));
		await rename(temporary, target);
	});
	writes = write.catch(() => {});
	await write;
	return target;
}

export async function cleanupInstances(instances: Instance[]): Promise<void> {
	cleaning = true;
	try {
		const manifest = await saveCleanupRegistry(instances);
		const result = await createCommand([
			"cleanup",
			"run",
			{ "--manifest": manifest, "--confirm": true, "--launcher-session": true },
		]).execute();
		const registry = JSON.parse(new TextDecoder().decode(await readFile(manifest))) as Registry;
		const remaining = new Set(registry.instances.map((i) => i.id));
		for (const instance of instances) {
			if (!remaining.has(instance.id)) {
				instanceMap.setKey(instance.id, undefined);
				restoreQueue.cancel(instance.path);
			}
		}
		if (result.code !== 0) throw new Error(result.stderr.trim() || "Instance cleanup failed.");
		instanceOrder.value = [];
		lastLaunchedInstanceId.value = undefined;
	} finally {
		cleaning = false;
	}
}

export function initializeCleanupRegistry(): () => void {
	let ready = $state(false);
	void (async () => {
		try {
			const manifest = await path.join(await appConfigDir(), "uninstall-instances.json");
			if (await exists(manifest)) {
				const registry = JSON.parse(
					new TextDecoder().decode(await readFile(manifest)),
				) as Registry;
				for (const id of registry.removedIds ?? []) {
					const instance = instanceMap.value[id];
					instanceMap.setKey(id, undefined);
					if (instance) restoreQueue.cancel(instance.path);
				}
			}
		} catch (error) {
			console.error("Failed to read cleanup registry:", error);
		}
		ready = true;
		cleanupRegistryState.ready = true;
	})();
	return $effect.root(() => {
		$effect(() => {
			if (!ready) return;
			const instances = Object.values(instanceMap.value).filter((i): i is Instance => !!i);
			const snapshot = JSON.parse(JSON.stringify(instances)) as Instance[];
			if (cleaning) return;
			void saveCleanupRegistry(snapshot).catch((error) =>
				addToast({
					title: "Could not register instances for uninstall",
					message: String(error),
					tone: "error",
				}),
			);
		});
	});
}
