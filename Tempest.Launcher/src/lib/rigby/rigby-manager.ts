import { createCommand } from "$lib/core/command";
import { appendProcessLog, logCommandOutput } from "$lib/stores/processes.svelte";
import { restoreError, restoreResult, restoreStatus } from "./stores.svelte";
import type { Child } from "$lib/platform/api";
import type { RestoreResult } from "./stores.svelte";

export interface RestoreOptions {
	manifests: string[];
	outDir: string;
	chunksRoot?: string;
	baseUrl?: string;
	noDownload?: boolean;
}

export class RigbyManager {
	private child: Child | null = null;
	// Incremented per restore run; close/error handlers of a superseded or
	// cancelled run must not overwrite the status afterwards.
	private runToken = 0;

	async restore(options: RestoreOptions): Promise<void> {
		await this.cancel();

		const token = ++this.runToken;

		restoreStatus.value = "running";
		restoreError.value = null;
		restoreResult.value = null;

		const args = [
			"rigby",
			"restore",
			options.manifests,
			"--out-dir",
			options.outDir,
			{ "--json": true },
			{ "--chunks-root": options.chunksRoot },
			{ "--base-url": options.baseUrl },
			{ "--no-download": options.noDownload },
		];

		appendProcessLog(`Restore args: ${JSON.stringify(args)}`, false, "rigby");

		try {
			const command = createCommand(args);
			logCommandOutput(command, "rigby");
			let stdout = "";
			let stderr = "";

			command.on("close", (data) => {
				if (this.runToken !== token) return; // cancelled or superseded
				if (data.code === 0) {
					try {
						const result = JSON.parse(stdout) as RestoreResult;
						restoreResult.value = result;
						restoreStatus.value = "complete";
					} catch {
						restoreError.value = `Failed to parse restore output: ${stdout}`;
						restoreStatus.value = "error";
					}
				} else {
					restoreError.value = stderr || `Restore failed with code ${data.code}`;
					restoreStatus.value = "error";
				}
			});

			command.on("error", (error) => {
				if (this.runToken !== token) return;
				restoreError.value = error;
				restoreStatus.value = "error";
			});

			command.stdout.on("data", (data) => {
				stdout += data;
			});

			command.stderr.on("data", (data) => {
				stderr += data;
			});

			const child = await command.spawn();
			// Cancelled while the spawn was in flight: kill it, don't adopt it.
			if (this.runToken !== token) {
				await child.kill().catch(() => {});
				return;
			}
			this.child = child;
		} catch (error) {
			if (this.runToken !== token) return;
			restoreError.value = String(error);
			restoreStatus.value = "error";
		}
	}

	async cancel(): Promise<void> {
		// Invalidate this run's handlers first, then kill the actual child
		// process. Aborting nothing was a no-op: the CLI kept downloading and
		// its close handler later resurrected "complete"/"error" state.
		this.runToken++;
		const child = this.child;
		this.child = null;
		if (child) {
			try {
				await child.kill();
			} catch (error) {
				console.error("Failed to kill restore process:", error);
			}
		}
		restoreStatus.value = "idle";
	}
}

export const rigbyManager = new RigbyManager();
