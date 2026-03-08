import { createCommand } from "$lib/core/command";
import { instanceMap, updateInstance } from "$lib/stores/instance";
import { queueCurrentIndex, queueItems, queueRunning } from "./stores";
import type { QueueItem } from "./stores";
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
		if (this.processing) return;
		queueRunning.set(true);
		this.processNext();
	}

	pause(): void {
		queueRunning.set(false);
	}

	private async processNext(): Promise<void> {
		if (!queueRunning.get() || this.processing) return;

		const items = queueItems.get();
		const nextIndex = items.findIndex((item) => item.status === "pending");

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
				console.error("Restore error:", error);
				this.updateItem(item.id, {
					status: "error",
					error: String(error),
					progress: undefined,
				});
			});

			command.on("close", (data) => {
				console.log(
					"Restore close:",
					data.code,
					"remaining stdout:",
					stdout,
					"stderr:",
					stderr,
				);

				if (data.code === 0) {
					const currentItem = queueItems.get().find((i) => i.id === item.id);
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
				} else {
					this.updateItem(item.id, {
						status: "error",
						error: stderr || `Failed with code ${data.code}`,
						progress: undefined,
					});
				}
			});

			await command.spawn();
		} catch (error) {
			console.error("Restore exception:", error);
			this.updateItem(item.id, {
				status: "error",
				error: String(error),
			});
		}
	}

	private updateInstanceState(outDir: string, stateType: string): void {
		const instances = instanceMap.get();
		const instance = Object.values(instances).find((inst) => inst?.path === outDir) as
			| Instance
			| undefined;
		if (instance?.id) {
			updateInstance(instance.id, {
				state: { type: stateType } as unknown as Instance["state"],
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
