import { invoke, type Command } from "$lib/platform/api";
import type { CrashSummary, GameLogFolders } from "$ipc/contract";
import type { Instance } from "$lib/types/instance";

// `[game-logs] <log folder>\t<temp folder>` from the CLI, once per launch.
const MARKER = /^\[game-logs\] (.+?)\t(.+?)\r?\n/m;

export const crashDialog = $state({
	value: { open: false, crash: null as CrashSummary | null, instance: null as Instance | null },
});

export function closeCrashDialog(): void {
	crashDialog.value = { open: false, crash: null, instance: null };
}

export function watchGameLogFolders(
	command: Command<string>,
	onFolders: (folders: GameLogFolders) => void,
): void {
	let buffer = "";
	const onData = (data: string) => {
		buffer += data;
		const match = MARKER.exec(buffer);
		if (match) {
			command.stderr.removeListener("data", onData);
			onFolders({ logs: match[1], temp: match[2] });
		} else {
			buffer = buffer.slice(buffer.lastIndexOf("\n") + 1);
		}
	};
	command.stderr.on("data", onData);
}

/**
 * Offers a report when the session that just ended crashed.
 * @param instance - The instance the game ran.
 * @param folders - Where the session wrote its logs.
 * @param startedAt - When it was launched (unix ms); older logs are other sessions'.
 * @param exitCode - The game's exit code.
 */
export async function checkForCrash(
	instance: Instance,
	folders: GameLogFolders,
	startedAt: number,
	exitCode: number | null,
): Promise<void> {
	try {
		const crash = await invoke("crash:inspect", {
			folders: { logs: folders.logs, temp: folders.temp },
			startedAt,
			exitCode,
		});
		if (!crash) return;
		crashDialog.value = { open: true, crash, instance };
		// The launcher may be in the tray while the game runs.
		await invoke("window:show");
		await invoke("window:set-focus");
	} catch (error) {
		console.error("Could not look for a game crash:", error);
	}
}
