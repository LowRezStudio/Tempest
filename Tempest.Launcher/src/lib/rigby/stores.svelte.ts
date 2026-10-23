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
	/** Checks an install already in the library rather than downloading a new one */
	repair?: boolean;
	status: "pending" | "running" | "paused" | "complete" | "error";
	result?: RestoreResult;
	progress?: ProgressEvent;
	error?: string;
	finishedAt?: number;
}

export interface SpeedSample {
	at: number;
	bytesPerSecond: number;
}

/** How far back speedSamples reaches, the window the downloads page graphs */
export const SPEED_WINDOW_MS = 120_000;

/** The running download's speed, about once a second; in memory only, so it restarts with the app */
export const speedSamples = $state({ value: [] as SpeedSample[] });

export const restoreStatus = $state({ value: "idle" as RestoreStatus });
export const restoreResult = $state({ value: null as RestoreResult | null });
export const restoreError = $state({ value: null as string | null });
export const restoreOutDir = $state({ value: "" });

export const queueItems = persistedJSON<QueueItem[]>("rigby:queue", []);
export const queueRunning = $state({ value: false });
export const queueCurrentIndex = $state({ value: null as number | null });

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
}
