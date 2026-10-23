import { hostState } from "$lib/stores/host-state.svelte";
import { persistedJSON } from "$lib/stores/persisted.svelte";

export type RestoreStatus = "idle" | "running" | "complete" | "error";

export interface RestoreResult {
	type: "complete";
	files: number;
	outDir: string;
	repairedFiles: number;
	verifiedFiles: number;
	deletedFiles: number;
	diskWriteBytes: number;
	reusedBytes: number;
}

export interface ProgressEvent {
	type: "progress";
	completedFiles: number;
	totalFiles: number;
	percent: number;
	completedBytes: number;
	totalBytes: number;
	bytesPerSecond: number;
	etaSeconds: number;
	repairedFiles: number;
	verifiedFiles: number;
	diskWriteBytes: number;
	reusedBytes: number;
}

export interface QueueItem {
	id: string;
	manifests: string[];
	outDir: string;
	chunksRoot?: string;
	baseUrl?: string;
	noDownload?: boolean;
	status: "pending" | "running" | "paused" | "complete" | "error";
	result?: RestoreResult;
	progress?: ProgressEvent;
	error?: string;
}

export const restoreStatus = hostState<RestoreStatus>("rigby:status", "idle");
export const restoreResult = hostState<RestoreResult | null>("rigby:result", null);
export const restoreError = hostState<string | null>("rigby:error", null);
export const restoreOutDir = hostState<string>("rigby:outDir", "");

export const queueItems = persistedJSON<QueueItem[]>("rigby:queue", []);
// Runtime queue state lives in the host: a reload (or a renderer crash) keeps
// the queue running instead of resetting it to nothing.
export const queueRunning = hostState<boolean>("rigby:running", false);
export const queueCurrentIndex = hostState<number | null>("rigby:currentIndex", null);
/** Item the queue is downloading right now, and the host child doing it. */
export const queueRunningId = hostState<string | null>("rigby:runningId", null);
export const queueRunningPid = hostState<number | null>("rigby:runningPid", null);

export const isRestoreRunning = {
	get value() {
		return restoreStatus.value === "running";
	},
};
export const hasRestoreError = {
	get value() {
		return restoreError.value !== null;
	},
};

export const queuePendingCount = {
	get value() {
		return queueItems.value.filter((i) => i.status === "pending").length;
	},
};
export const queuePausedCount = {
	get value() {
		return queueItems.value.filter((i) => i.status === "paused").length;
	},
};
export const queueCompletedCount = {
	get value() {
		return queueItems.value.filter((i) => i.status === "complete").length;
	},
};
export const queueErrorCount = {
	get value() {
		return queueItems.value.filter((i) => i.status === "error").length;
	},
};

export function resetRestoreState(): void {
	restoreStatus.value = "idle";
	restoreResult.value = null;
	restoreError.value = null;
}

export function resetQueueState(): void {
	queueItems.value = [];
	queueRunning.value = false;
	queueCurrentIndex.value = null;
	queueRunningId.value = null;
	queueRunningPid.value = null;
}
