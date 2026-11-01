// Studio updates, as the launcher has them: a check at start (unless turned
// off in Settings), a banner when a newer Studio is published, download with
// progress and install-and-restart where the Studio can update itself
// (Windows, Linux AppImage/deb/rpm), else a link to the release page (macOS).
// Unsaved work is saved (project) or offered a workspace file first.
import { invoke, listen } from "$lib/platform/api";
import { assembly } from "./assembly.svelte";
import { commands } from "./commands.svelte";
import { session } from "./session.svelte";
import { studio } from "./state.svelte";
import { toasts } from "./toast.svelte";
import { ui } from "./ui.svelte";
import type { StudioUpdate } from "../../../electron/src/ipc/contract";

const AUTO_KEY = "studio.updates.auto";

export type UpdateStatus =
	| "idle"
	| "checking"
	| "up-to-date"
	| "available"
	| "downloading"
	| "ready"
	| "installing"
	| "error";

class Updater {
	status = $state<UpdateStatus>("idle");
	update = $state<StudioUpdate | null>(null);
	percent = $state(0);
	error = $state<string | null>(null);
	/** The banner was closed for this version. */
	dismissed = $state(false);
	/** Check for a newer Studio when it starts. */
	auto = $state(readAuto());

	setAuto(on: boolean): void {
		this.auto = on;
		try {
			localStorage.setItem(AUTO_KEY, on ? "1" : "0");
		} catch {
			// stays for this session
		}
	}

	async check(silent = false): Promise<void> {
		if (this.status === "checking" || this.status === "downloading") return;
		this.status = "checking";
		this.error = null;
		try {
			const r = await invoke("updater:check");
			this.update = r;
			this.status = r.available ? "available" : "up-to-date";
			this.dismissed = false;
		} catch (error) {
			this.status = "error";
			this.error = error instanceof Error ? error.message : String(error);
			if (!silent) toasts.error(error, "Could not check for updates");
		}
	}

	async download(): Promise<void> {
		if (!this.update?.canInstall) return;
		this.status = "downloading";
		this.percent = 0;
		const stop = listen("updater:progress", (p) => (this.percent = Math.round(p.percent)));
		try {
			await invoke("updater:download");
			this.status = "ready";
		} catch (error) {
			this.status = "error";
			this.error = error instanceof Error ? error.message : String(error);
			toasts.error(error, "The update did not download");
		} finally {
			stop();
		}
	}

	/** Install and restart, once unsaved work is safe. */
	async install(): Promise<void> {
		if (this.status !== "ready") return;
		if (!(await this.workIsSafe())) return;
		this.status = "installing";
		try {
			await invoke("updater:install");
		} catch (error) {
			this.status = "ready";
			toasts.error(error, "The update was not installed");
		}
	}

	/** A project saves; loose unsaved edits get a workspace file first, if wanted. */
	private async workIsSafe(): Promise<boolean> {
		if (session.project) {
			await session.saveProject();
			return session.projectState !== "error";
		}
		const unsaved = studio.dirty.length + assembly.dirtyDocs.length;
		if (unsaved === 0) return true;
		const ok = await ui.confirm({
			title: "Restart with unsaved edits?",
			message: `${unsaved} file${unsaved === 1 ? " has" : "s have"} unsaved edits, which are lost when the Studio restarts. Keep them in a workspace file first to reopen them after the update.`,
			confirm: "Restart and lose them",
			alt: "Save a workspace file first",
			danger: true,
		});
		if (ok === "alt") {
			await commands.get("workspace.save")?.run();
			return studio.dirty.length + assembly.dirtyDocs.length === 0 || !!session.file;
		}
		return ok === true;
	}
}

function readAuto(): boolean {
	try {
		return localStorage.getItem(AUTO_KEY) !== "0";
	} catch {
		return true;
	}
}

export const updater = new Updater();
