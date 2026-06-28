import { createCommand } from "$lib/core/command";
import { appendProcessLog, logCommandOutput } from "$lib/stores/processes.svelte";
import { restoreError, restoreResult, restoreStatus } from "./stores.svelte";
import type { RestoreResult } from "./stores.svelte";

export interface RestoreOptions {
	manifests: string[];
	outDir: string;
	chunksRoot?: string;
	baseUrl?: string;
	noDownload?: boolean;
}

export class RigbyManager {
	private abortController: AbortController | null = null;

	async restore(options: RestoreOptions): Promise<void> {
		this.abortController?.abort();
		this.abortController = new AbortController();

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
				this.abortController = null;
			});

			command.on("error", (error) => {
				restoreError.value = error;
				restoreStatus.value = "error";
				this.abortController = null;
			});

			command.stdout.on("data", (data) => {
				stdout += data;
			});

			command.stderr.on("data", (data) => {
				stderr += data;
			});

			await command.spawn();
		} catch (error) {
			restoreError.value = String(error);
			restoreStatus.value = "error";
			this.abortController = null;
		}
	}

	cancel(): void {
		this.abortController?.abort();
		this.abortController = null;
		restoreStatus.value = "idle";
	}
}

export const rigbyManager = new RigbyManager();
