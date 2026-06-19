import { createCommand } from "$lib/core/command";
import { instanceMap, updateInstance } from "$lib/stores/instance";
import { logCommandOutput } from "$lib/stores/processes";
import { queueCurrentIndex, queueItems, queueRunning } from "./stores";
import type { QueueItem } from "./stores";
import type { Child } from "@tauri-apps/plugin-shell";
import type { ArgumentType } from "$lib/core/command";
import type { Instance } from "$lib/types/instance";

function generateId(): string {
	return `restore-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

function resetRunningItems(): void {
	const items = queueItems.get();
	const hasRunning = items.some((item) => item.status === "running");
	if (!hasRunning) return;

	const updated = items.map((item) =>
		item.status === "running" ?
			{ ...item, status: "pending" as const, progress: undefined }
		:	item,
	);
	queueItems.set(updated);
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
		queueItems.set([...queueItems.get(), item]);

		if (!this.processing && !queueRunning.get()) {
			this.start();
		}

		return item.id;
	}

	remove(id: string): void {
		const items = queueItems.get();
		const index = items.findIndex((item) => item.id === id);
		if (index === -1) return;

		const item = items[index];
		if (item.status === "running") return;

		const newItems = items.filter((i) => i.id !== id);
		queueItems.set(newItems);

		const currentIdx = queueCurrentIndex.get();
		if (currentIdx !== null && index < currentIdx) {
			queueCurrentIndex.set(currentIdx - 1);
		}
	}

	reorder(fromIndex: number, toIndex: number): void {
		const items = queueItems.get();
		const pendingItems = items.filter((i) => i.status === "pending");
		const completedItems = items.filter((i) => i.status !== "pending");

		if (fromIndex >= pendingItems.length || toIndex >= pendingItems.length) {
			return;
		}

		const [moved] = pendingItems.splice(fromIndex, 1);
		pendingItems.splice(toIndex, 0, moved);

		queueItems.set([...completedItems, ...pendingItems]);
	}

	clearCompleted(): void {
		const items = queueItems.get();
		queueItems.set(items.filter((item) => item.status !== "complete"));
		queueCurrentIndex.set(null);
	}

	start(): void {
		if (this.pausing) return;
		queueRunning.set(true);
		if (!this.processing) {
			this.processNext();
		}
	}

	async pause(): Promise<void> {
		queueRunning.set(false);

		if (this.runningChild && this.runningItemId) {
			this.pausing = true;
			const child = this.runningChild;
			const itemId = this.runningItemId;
			const item = queueItems.get().find((i) => i.id === itemId);

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

		const items = queueItems.get();
		const newItems = items.filter((item) => item.outDir !== outDir);
		queueItems.set(newItems);

		const instance = Object.values(instanceMap.get()).find((inst) => inst?.path === outDir) as
			| Instance
			| undefined;
		if (instance?.id) {
			updateInstance(instance.id, {
				state: { type: "prepared" },
			});
		}

		if (this.processing) {
			this.processing = false;
			this.processNext();
		}
	}

	private async processNext(): Promise<void> {
		if (!queueRunning.get() || this.processing) return;

		const items = queueItems.get();
		const nextIndex = items.findIndex(
			(item) => item.status === "pending" || item.status === "paused",
		);

		if (nextIndex === -1) {
			queueRunning.set(false);
			queueCurrentIndex.set(null);
			this.processing = false;
			return;
		}

		this.processing = true;
		queueCurrentIndex.set(nextIndex);

		const item = items[nextIndex];
		this.updateItem(item.id, { status: "running" });
		this.updateInstanceState(item.outDir, "downloading");

		await this.runRestore(item);

		this.processing = false;

		if (queueRunning.get()) {
			this.processNext();
		}
	}

	private async runRestore(item: QueueItem): Promise<void> {
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

		console.log("Restore args:", JSON.stringify(args, null, 2));

		try {
			const command = createCommand(args);
			logCommandOutput(command, "rigby-queue");
			this.runningOutDir = item.outDir;
			this.runningItemId = item.id;
			let stdout = "";
			let stderr = "";

			command.stdout.on("data", (data) => {
				stdout += data;
				console.log("stdout chunk:", data);

				const lines = stdout.split("\n");
				stdout = lines.pop() || "";

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
					return;
				}

				const currentItem = queueItems.get().find((i) => i.id === item.id);
				if (currentItem?.status === "paused") {
					console.log("Ignoring error because restore was paused");
					return;
				}

				console.error("Restore error:", error);
				this.updateItem(item.id, {
					status: "error",
					error: String(error),
					progress: undefined,
				});
			});

			command.on("close", (data) => {
				if (this.runningItemId !== item.id) {
					console.log("Ignoring stale close event");
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

				const currentItem = queueItems.get().find((i) => i.id === item.id);

				if (data.code === 0) {
					if (currentItem?.status === "complete") {
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
				} else if (currentItem?.status === "paused") {
					console.log("Ignoring close error because restore was paused");
				} else {
					this.updateItem(item.id, {
						status: "error",
						error: stderr || `Failed with code ${data.code}`,
						progress: undefined,
					});
				}
			});

			this.runningChild = await command.spawn();
		} catch (error) {
			console.error("Restore exception:", error);
			this.runningChild = null;
			this.runningOutDir = null;
			this.runningItemId = null;
			this.updateItem(item.id, {
				status: "error",
				error: String(error),
			});
		}
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

	private updateItem(
		id: string,
		updates: Partial<Pick<QueueItem, "status" | "result" | "error" | "progress">>,
	): void {
		const items = queueItems.get();
		const index = items.findIndex((item) => item.id === id);
		if (index === -1) return;

		const newItems = [...items];
		newItems[index] = { ...newItems[index], ...updates };
		queueItems.set(newItems);
	}
}

export const restoreQueue = new RestoreQueue();
