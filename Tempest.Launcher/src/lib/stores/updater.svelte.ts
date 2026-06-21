import { invoke } from "@tauri-apps/api/core";
import { check } from "@tauri-apps/plugin-updater";
import { triggerChildCleanup } from "$lib/tauri/scopes";
import type { Update } from "@tauri-apps/plugin-updater";

export type UpdaterStatus =
	| "idle"
	| "checking"
	| "available"
	| "up-to-date"
	| "downloading"
	| "installing"
	| "error"
	| "relaunching";

class UpdaterStore {
	status = $state<UpdaterStatus>("idle");
	update = $state<Update | null>(null);
	contentLength = $state<number | null>(null);
	downloadedBytes = $state<number>(0);
	errorMessage = $state<string | null>(null);

	private _isOpen = $state<boolean>(false);

	get isOpen(): boolean {
		return this._isOpen;
	}

	set isOpen(value: boolean) {
		if (this._isOpen && !value) {
			// ponytail: transition from open to closed triggers cleanup on Windows if not updating
			if (
				this.status === "available" ||
				this.status === "error" ||
				this.status === "up-to-date" ||
				this.status === "idle"
			) {
				triggerChildCleanup().catch((error) =>
					console.error("Failed to trigger child cleanup on close:", error),
				);
			}
		}
		this._isOpen = value;
	}

	get progress(): number {
		if (!this.contentLength) return 0;
		return Math.round((this.downloadedBytes / this.contentLength) * 100);
	}

	async checkForUpdates(silent = false) {
		this.errorMessage = null;
		this.status = "checking";
		if (!silent) {
			this.isOpen = true;
		}

		try {
			// ponytail: check() can throw if updater is not properly configured/built, we catch and handle it safely.
			const res = await check();
			this.update = res;

			if (res) {
				this.status = "available";
				this.isOpen = true;
			} else {
				this.status = "up-to-date";
				if (silent) {
					this.isOpen = false;
				}
				// ponytail: no update available, so trigger cleanup immediately
				await triggerChildCleanup();
			}
		} catch (error: any) {
			console.error("Failed to check for updates:", error);
			this.errorMessage = error?.message || String(error);
			this.status = "error";
			if (silent) {
				// Don't show error dialog if checking silently on start
				this.isOpen = false;
			}
			// ponytail: check failed, trigger cleanup to be safe
			await triggerChildCleanup();
		}
	}

	async startUpdate() {
		if (!this.update) return;

		this.status = "downloading";
		this.downloadedBytes = 0;
		this.contentLength = null;

		try {
			await this.update.downloadAndInstall((event) => {
				switch (event.event) {
					case "Started": {
						this.contentLength = event.data.contentLength ?? null;
						break;
					}
					case "Progress": {
						this.downloadedBytes += event.data.chunkLength;
						break;
					}
					case "Finished": {
						this.status = "installing";
						break;
					}
				}
			});

			this.status = "relaunching";
			// ponytail: relaunch app after small delay to let user see "Finished/Relaunching" status
			setTimeout(async () => {
				try {
					await invoke("relaunch");
				} catch (error) {
					console.error("Relaunch command failed:", error);
					this.status = "error";
					this.errorMessage =
						"Failed to restart application automatically. Please restart manually.";
				}
			}, 1000);
		} catch (error: any) {
			console.error("Update download/install failed:", error);
			this.errorMessage = error?.message || String(error);
			this.status = "error";
		}
	}
}

export const updaterStore = new UpdaterStore();
