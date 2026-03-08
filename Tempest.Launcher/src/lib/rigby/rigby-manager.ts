import { createCommand } from "$lib/core/command";
import { restoreError, restoreResult, restoreStatus } from "./stores";
import type { RestoreResult } from "./stores";

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

		restoreStatus.set("running");
		restoreError.set(null);
		restoreResult.set(null);

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

		try {
			const command = createCommand(args);
			let stdout = "";
			let stderr = "";

			command.on("close", (data) => {
				if (data.code === 0) {
					try {
						const result = JSON.parse(stdout) as RestoreResult;
						restoreResult.set(result);
						restoreStatus.set("complete");
					} catch {
						restoreError.set(`Failed to parse restore output: ${stdout}`);
						restoreStatus.set("error");
					}
				} else {
					restoreError.set(stderr || `Restore failed with code ${data.code}`);
					restoreStatus.set("error");
				}
				this.abortController = null;
			});

			command.on("error", (error) => {
				restoreError.set(error);
				restoreStatus.set("error");
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
			restoreError.set(String(error));
			restoreStatus.set("error");
			this.abortController = null;
		}
	}

	cancel(): void {
		this.abortController?.abort();
		this.abortController = null;
		restoreStatus.set("idle");
	}
}

export const rigbyManager = new RigbyManager();
