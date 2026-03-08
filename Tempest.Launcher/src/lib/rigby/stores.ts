import { persistentAtom } from "@nanostores/persistent";
import { atom, computed } from "nanostores";

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
	status: "pending" | "running" | "complete" | "error";
	result?: RestoreResult;
	progress?: ProgressEvent;
	error?: string;
}

export const restoreStatus = atom<RestoreStatus>("idle");
export const restoreResult = atom<RestoreResult | null>(null);
export const restoreError = atom<string | null>(null);
export const restoreOutDir = atom<string>("");

export const queueItems = persistentAtom<QueueItem[]>("rigby:queue", [], {
	encode: JSON.stringify,
	decode: JSON.parse,
});
export const queueRunning = atom<boolean>(false);
export const queueCurrentIndex = atom<number | null>(null);

export const isRestoreRunning = computed(restoreStatus, ($status) => $status === "running");
export const hasRestoreError = computed(restoreError, ($error) => $error !== null);

export const queuePendingCount = computed(
	queueItems,
	($items) => $items.filter((i) => i.status === "pending").length,
);
export const queueCompletedCount = computed(
	queueItems,
	($items) => $items.filter((i) => i.status === "complete").length,
);
export const queueErrorCount = computed(
	queueItems,
	($items) => $items.filter((i) => i.status === "error").length,
);

export function resetRestoreState(): void {
	restoreStatus.set("idle");
	restoreResult.set(null);
	restoreError.set(null);
}

export function resetQueueState(): void {
	queueItems.set([]);
	queueRunning.set(false);
	queueCurrentIndex.set(null);
}
