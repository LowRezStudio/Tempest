import { invoke } from "@tauri-apps/api/core";
import { processesList } from "$lib/stores/processes.svelte";

const PROCESS_CHECK_INTERVAL_MS = 2000;

let reconciliationInProgress = false;
let reportedCheckFailure = false;

async function reconcileProcesses(): Promise<void> {
	if (reconciliationInProgress || processesList.value.length === 0) return;
	reconciliationInProgress = true;

	try {
		const snapshot = processesList.value;
		const states = await Promise.all(
			snapshot.map(async (process) => {
				try {
					return await invoke<boolean>("is_process_running", { pid: process.child.pid });
				} catch (error) {
					if (!reportedCheckFailure) {
						console.warn("Unable to reconcile launched process state", error);
						reportedCheckFailure = true;
					}
					return true;
				}
			}),
		);

		const stoppedPids = new Set(
			snapshot.filter((_, index) => !states[index]).map((process) => process.child.pid),
		);
		if (stoppedPids.size > 0) {
			processesList.value = processesList.value.filter(
				(process) => !stoppedPids.has(process.child.pid),
			);
		}
	} finally {
		reconciliationInProgress = false;
	}
}

export function startProcessMonitor(): () => void {
	const check = () => void reconcileProcesses();
	const checkWhenVisible = () => {
		if (document.visibilityState === "visible") check();
	};

	check();
	const interval = window.setInterval(check, PROCESS_CHECK_INTERVAL_MS);
	window.addEventListener("focus", check);
	document.addEventListener("visibilitychange", checkWhenVisible);

	return () => {
		window.clearInterval(interval);
		window.removeEventListener("focus", check);
		document.removeEventListener("visibilitychange", checkWhenVisible);
	};
}
