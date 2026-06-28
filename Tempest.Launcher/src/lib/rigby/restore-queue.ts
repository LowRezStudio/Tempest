import { createCommand } from "$lib/core/command";
import { m } from "$lib/paraglide/messages";
import { instanceMap, updateInstance } from "$lib/stores/instance.svelte";
import { appendProcessLog, logCommandOutput } from "$lib/stores/processes.svelte";
import { addToast } from "$lib/stores/ui.svelte";
import { queueCurrentIndex, queueItems, queueRunning } from "./stores.svelte";
import type { ArgumentType } from "$lib/core/command";
import type { Instance } from "$lib/types/instance";
import type { QueueItem } from "./stores.svelte";
import type { Child } from "@tauri-apps/plugin-shell";

function generateId(): string {
	return `restore-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

function resetRunningItems(): void {
	const items = queueItems.value;
	const hasRunning = items.some((item) => item.status === "running");
	if (!hasRunning) return;

	const updated = items.map((item) =>
		item.status === "running"
			? { ...item, status: "pending" as const, progress: undefined }
			: item,
	);
	queueItems.value = updated;
}

resetRunningItems();

export class RestoreQueue {
	private processing = false;
	private pausing = false;
	private runningChild: Child | null = null;
	private runningOutDir: string | null = null;
	private runningItemId: string | null = null;

	add(options: Omit<QueueItem, "id" | "status" | "result" | "error">): string {
		const item: QueueItem = {
			...options,
			id: generateId(),
			status: "pending",
		};
		queueItems.value = [...queueItems.value, item];

		if (!this.processing && !queueRunning.value) {
			this.start();
		}

		return item.id;
	}

	remove(id: string): void {
		const items = queueItems.value;
		const index = items.findIndex((item) => item.id === id);
		if (index === -1) return;

		const item = items[index];
		if (item.status === "running") return;

		const newItems = items.filter((i) => i.id !== id);
		queueItems.value = newItems;

		const currentIdx = queueCurrentIndex.value;
		if (currentIdx !== null && index < currentIdx) {
			queueCurrentIndex.value = currentIdx - 1;
		}
	}

	reorder(fromIndex: number, toIndex: number): void {
		const items = queueItems.value;
		const pendingItems = items.filter((i) => i.status === "pending");
		const completedItems = items.filter((i) => i.status !== "pending");

		if (fromIndex >= pendingItems.length || toIndex >= pendingItems.length) {
			return;
		}

		const [moved] = pendingItems.splice(fromIndex, 1);
		pendingItems.splice(toIndex, 0, moved);

		queueItems.value = [...completedItems, ...pendingItems];
	}

	clearCompleted(): void {
		const items = queueItems.value;
		queueItems.value = items.filter((item) => item.status !== "complete");
		queueCurrentIndex.value = null;
	}

	start(): void {
		if (this.pausing) return;
		queueRunning.value = true;
		if (!this.processing) {
			void this.processNext();
		}
	}

	async pause(): Promise<void> {
		queueRunning.value = false;

		if (this.runningChild && this.runningItemId) {
			this.pausing = true;
			const child = this.runningChild;
			const itemId = this.runningItemId;
			const item = queueItems.value.find((i) => i.id === itemId);

			this.updateItem(itemId, { status: "paused", progress: undefined });
			if (item) {
				this.updateInstanceState(item.outDir, "paused");
			}

			try {
				await child.kill();
			} catch (error) {
				console.error("Failed to kill restore process:", error);
			} finally {
				this.runningChild = null;
				this.runningOutDir = null;
				this.runningItemId = null;
				this.pausing = false;
			}
		}
	}

	cancel(outDir: string): void {
		if (this.runningOutDir === outDir && this.runningChild) {
			void this.runningChild.kill();
			this.runningChild = null;
			this.runningOutDir = null;
			this.runningItemId = null;
		}

		const items = queueItems.value;
		const newItems = items.filter((item) => item.outDir !== outDir);
		queueItems.value = newItems;

		const instance = Object.values(instanceMap.get()).find((inst) => inst?.path === outDir) as
			| Instance
			| undefined;
		if (instance?.id) {
			updateInstance(instance.id, {
				state: { type: "prepared" },
			});
		}
	}

	retry(id: string): void {
		const items = queueItems.value;
		const item = items.find((i) => i.id === id);
		if (!item || item.status !== "error") return;

		this.updateItem(id, {
			status: "pending",
			error: undefined,
			progress: undefined,
			result: undefined,
		});
		this.start();
	}

	resume(id: string): void {
		const items = queueItems.value;
		const item = items.find((i) => i.id === id);
		if (!item || item.status !== "paused") return;

		const otherItems = items.filter((i) => i.id !== id);
		const allOthersPaused = otherItems.every((i) => i.status === "paused");

		if (allOthersPaused) {
			queueItems.value = [{ ...item, status: "pending", progress: undefined }, ...otherItems];
		} else {
			this.updateItem(id, { status: "pending", progress: undefined });
		}

		this.start();
	}

	private async processNext(): Promise<void> {
		if (!queueRunning.value || this.processing) return;

		const items = queueItems.value;
		const nextIndex = items.findIndex(
			(item) => item.status === "pending" || item.status === "paused",
		);

		if (nextIndex === -1) {
			queueRunning.value = false;
			queueCurrentIndex.value = null;
			this.processing = false;
			return;
		}

		this.processing = true;
		queueCurrentIndex.value = nextIndex;

		const item = items[nextIndex];
		this.updateItem(item.id, { status: "running" });
		this.updateInstanceState(item.outDir, "downloading");

		try {
			await this.runRestore(item);
		} catch (error) {
			console.error("Unexpected restore failure:", error);
			this.updateItem(item.id, {
				status: "error",
				error: String(error),
				progress: undefined,
			});
			this.updateInstanceState(item.outDir, "prepared");
		} finally {
			this.processing = false;
		}

		if (queueRunning.value) {
			void this.processNext();
		}
	}

	private runRestore(item: QueueItem): Promise<void> {
		const args = [
			"rigby",
			"restore",
			...item.manifests,
			"--out-dir",
			item.outDir,
			{ "--json": true },
		] as ArgumentType[];

		if (item.chunksRoot) {
			args.push({ "--chunks-root": item.chunksRoot });
		}
		if (item.baseUrl) {
			args.push({ "--base-url": item.baseUrl });
		}
		if (item.noDownload) {
			args.push({ "--no-download": true });
		}

		appendProcessLog(`Restore args: ${JSON.stringify(args)}`, false, "rigby-queue");

		const command = createCommand(args);
		logCommandOutput(command, "rigby-queue");
		this.runningOutDir = item.outDir;
		this.runningItemId = item.id;
		let stdout = "";
		let stderr = "";

		return new Promise<void>((resolve) => {
			let settled = false;

			const finish = () => {
				if (settled) return;
				settled = true;
				this.runningChild = null;
				this.runningOutDir = null;
				this.runningItemId = null;
				resolve();
			};

			command.stdout.on("data", (data) => {
				stdout += data;
				console.log("stdout chunk:", data);

				const lines = stdout.split("\n");
				stdout = lines.pop() ?? "";

				for (const line of lines) {
					if (!line.trim()) continue;
					try {
						const parsed = JSON.parse(line);
						if (parsed.type === "progress") {
							this.updateItem(item.id, { progress: parsed });
						} else if (parsed.type === "complete") {
							this.updateItem(item.id, {
								status: "complete",
								result: parsed,
								progress: undefined,
							});
							this.updateInstanceState(item.outDir, "prepared");
						}
					} catch {
						console.warn("Failed to parse JSON line:", line);
					}
				}
			});

			command.stderr.on("data", (data) => {
				stderr += data;
				console.log("stderr chunk:", data);
			});

			command.on("error", (error) => {
				if (this.runningItemId !== item.id) {
					console.log("Ignoring stale error event");
					finish();
					return;
				}

				const currentItem = queueItems.value.find((i) => i.id === item.id);
				if (currentItem?.status === "paused") {
					console.log("Ignoring error because restore was paused");
					finish();
					return;
				}

				console.error("Restore error:", error);
				this.updateItem(item.id, {
					status: "error",
					error: String(error),
					progress: undefined,
				});
				this.updateInstanceState(item.outDir, "prepared");
				finish();
			});

			command.on("close", (data) => {
				if (this.runningItemId !== item.id) {
					console.log("Ignoring stale close event");
					finish();
					return;
				}

				this.runningChild = null;
				this.runningOutDir = null;
				this.runningItemId = null;
				console.log(
					"Restore close:",
					data.code,
					"remaining stdout:",
					stdout,
					"stderr:",
					stderr,
				);

				const currentItem = queueItems.value.find((i) => i.id === item.id);

				if (data.code === 0) {
					if (currentItem?.status === "complete") {
						finish();
						return;
					}

					if (stdout.trim()) {
						try {
							const parsed = JSON.parse(stdout);
							if (parsed.type === "complete") {
								this.updateItem(item.id, {
									status: "complete",
									result: parsed,
									progress: undefined,
								});
								this.updateInstanceState(item.outDir, "prepared");
								finish();
								return;
							}
						} catch {
							console.warn("Failed to parse remaining stdout:", stdout);
						}
					}

					this.updateItem(item.id, {
						status: "error",
						error: "Process completed but no completion event was received",
					});
					this.updateInstanceState(item.outDir, "prepared");
					finish();
				} else if (currentItem?.status === "paused") {
					console.log("Ignoring close error because restore was paused");
					finish();
				} else {
					this.updateItem(item.id, {
						status: "error",
						error: stderr || `Failed with code ${data.code}`,
						progress: undefined,
					});
					this.updateInstanceState(item.outDir, "prepared");
					finish();
				}
			});

			void command
				.spawn()
				.then((child) => {
					this.runningChild = child;
				})
				.catch((error) => {
					console.error("Restore exception:", error);
					this.updateItem(item.id, {
						status: "error",
						error: String(error),
					});
					this.updateInstanceState(item.outDir, "prepared");
					finish();
				});
		});
	}

	private updateInstanceState(outDir: string, stateType: Instance["state"]["type"]): void {
		const instances = instanceMap.get();
		const instance = Object.values(instances).find((inst) => inst?.path === outDir) as
			| Instance
			| undefined;
		if (instance?.id) {
			updateInstance(instance.id, {
				state: { type: stateType },
			});
		}
	}

	private instanceName(outDir: string): string {
		const instances = instanceMap.get();
		const instance = Object.values(instances).find((inst) => inst?.path === outDir) as
			| Instance
			| undefined;
		return instance?.label ?? outDir.split(/[/\\]/).pop() ?? outDir;
	}

	private notifyStatusChange(item: QueueItem): void {
		const name = this.instanceName(item.outDir);

		if (item.status === "running") {
			addToast({
				title: m.toast_download_started_title(),
				message: m.toast_download_started_message({ name }),
				tone: "info",
			});
		} else if (item.status === "complete") {
			addToast({
				title: m.toast_download_finished_title(),
				message: m.toast_download_finished_message({ name }),
				tone: "success",
			});
		} else if (item.status === "error") {
			addToast({
				title: m.toast_download_failed_title(),
				message: `${name}: ${item.error ?? m.toast_download_failed_unknown()}`,
				tone: "error",
			});
		}
	}

	private updateItem(
		id: string,
		updates: Partial<Pick<QueueItem, "status" | "result" | "error" | "progress">>,
	): void {
		const items = queueItems.value;
		const index = items.findIndex((item) => item.id === id);
		if (index === -1) return;

		const oldStatus = items[index].status;
		const newItems = [...items];
		newItems[index] = { ...newItems[index], ...updates };
		queueItems.value = newItems;

		if (updates.status && updates.status !== oldStatus) {
			this.notifyStatusChange(newItems[index]);
		}
	}
}

export const restoreQueue = new RestoreQueue();
