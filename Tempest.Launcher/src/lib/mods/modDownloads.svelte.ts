import { SvelteMap } from "svelte/reactivity";
import { SPEED_SAMPLE_MS } from "$lib/rigby/stores.svelte";
import { instanceMap } from "$lib/stores/instance.svelte";
import { persistedJSON } from "$lib/stores/persisted.svelte";
import { patchSummary } from "$lib/utils/versions";
import type { QueueItem, SpeedSample } from "$lib/rigby/stores.svelte";

// Mod packages downloaded from the repository, shaped like restore queue items so the Downloads
// page lists them with the patches. Kept out of that queue, which runs whatever it holds.
const HISTORY = 100;
const finished = persistedJSON<QueueItem[]>("modDownloads", []);
const running = $state<{ value: QueueItem[] }>({ value: [] });
const samples = new SvelteMap<string, SpeedSample[]>();

export const modDownloadItems = {
	get value() {
		return [...running.value, ...finished.value];
	},
};

export const modSpeedSamples = (id: string) => samples.get(id) ?? [];

export function removeModDownload(id: string) {
	finished.value = finished.value.filter((item) => item.id !== id);
}

export function clearModDownloads() {
	finished.value = [];
}

export function trackModDownload(gamePath: string, modId: string, name: string, version: string) {
	const instance = Object.values(instanceMap.value).find((i) => i?.path === gamePath);
	const item: QueueItem = {
		id: crypto.randomUUID(),
		manifests: [],
		outDir: gamePath,
		status: "running",
		mod: {
			modId,
			name,
			version,
			patchLabel: instance && patchSummary(instance),
		},
	};
	const { id } = item;
	const startedAt = Date.now();
	let last = { at: startedAt, received: 0 };
	let speed = 0;
	let size = 0;
	const addSample = (at: number, bytesPerSecond: number) =>
		samples.set(id, [...modSpeedSamples(id), { at, bytesPerSecond }]);

	return {
		progress(received: number, total: number | null) {
			const now = Date.now();
			if (size === 0) {
				running.value = [...running.value, item];
				addSample(startedAt, 0);
			}
			size = total ?? received;
			if (now - last.at >= SPEED_SAMPLE_MS) {
				speed = ((received - last.received) / (now - last.at)) * 1000;
				last = { at: now, received };
				addSample(now, speed);
			}
			const progress: QueueItem["progress"] = {
				type: "progress",
				completedFiles: 0,
				totalFiles: 1,
				percent: total ? (received / total) * 100 : 0,
				completedBytes: received,
				totalBytes: size,
				bytesPerSecond: speed,
				etaSeconds: speed > 0 && total ? (total - received) / speed : 0,
				repairedFiles: 0,
				verifiedFiles: 0,
				diskWriteBytes: received,
				reusedBytes: 0,
			};
			running.value = running.value.map((r) => (r.id === id ? { ...r, progress } : r));
		},
		setInstalling(percent: number | null = null) {
			running.value = running.value.map((r) =>
				r.id === id && r.mod ? { ...r, mod: { ...r.mod, install: { percent } } } : r,
			);
		},
		finish(succeeded: boolean) {
			running.value = running.value.filter((r) => r.id !== id);
			samples.delete(id);
			if (!succeeded || size === 0) return;
			const now = Date.now();
			const done: QueueItem = {
				...item,
				status: "complete",
				finishedAt: now,
				activeMs: now - startedAt,
				result: {
					type: "complete",
					files: 1,
					outDir: gamePath,
					repairedFiles: 0,
					verifiedFiles: 0,
					deletedFiles: 0,
					diskWriteBytes: size,
					reusedBytes: 0,
				},
			};
			finished.value = [done, ...finished.value].slice(0, HISTORY);
		},
	};
}

export type ModDownloadTracker = ReturnType<typeof trackModDownload>;
