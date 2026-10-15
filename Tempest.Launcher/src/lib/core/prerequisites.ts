import { createCommand } from "$lib/core/command";
import { appendProcessLogs } from "$lib/stores/processes.svelte";
import { addToast } from "$lib/stores/ui.svelte";

let queue = Promise.resolve();

export function installPrerequisites(gamePath: string, force = false): Promise<void> {
	const operation = queue.then(async () => {
		addToast({
			title: "Installing game prerequisites",
			message: "Complete the runtime installer dialogs to continue.",
			tone: "info",
		});
		const result = await createCommand([
			"build",
			"install-prerequisites",
			gamePath,
			{ "--force": force },
		]).execute();
		if (result.stdout) {
			appendProcessLogs(result.stdout.split("\n").filter(Boolean), false, "setup");
		}
		if (result.code !== 0) {
			throw new Error(result.stderr.trim() || "Game prerequisite installation failed.");
		}
	});
	queue = operation.catch(() => {});
	return operation;
}
