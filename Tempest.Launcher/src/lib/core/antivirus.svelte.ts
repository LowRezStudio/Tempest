import type { Command } from "$lib/platform/api";
import type { Instance } from "$lib/types/instance";

// The CLI prints `[av-blocked] <folder>` when an antivirus blocks the injector or a mod DLL.
const MARKER = /^\[av-blocked\] (.+?)\r?$/gm;

/** `instance` is set when a game launch was stopped over the block, so the dialog can relaunch it. */
export const antivirusDialog = $state({
	value: { open: false, folders: [] as string[], instance: null as Instance | null },
});

export function closeAntivirusDialog(): void {
	antivirusDialog.value = { open: false, folders: [], instance: null };
}

export function reportAntivirusBlocks(output: string, instance?: Instance): boolean {
	const found = [...output.matchAll(MARKER)].map((match) => match[1]);
	if (found.length === 0) return false;
	const current = antivirusDialog.value.open ? antivirusDialog.value : null;
	antivirusDialog.value = {
		open: true,
		folders: [...new Set([...(current?.folders ?? []), ...found])],
		instance: instance ?? current?.instance ?? null,
	};
	return true;
}

export function watchAntivirusBlocks(
	command: Command<string>,
	options: { instance?: Instance; onBlocked?: () => void } = {},
): void {
	let buffer = "";
	let blocked = false;
	command.stderr.on("data", (data) => {
		const lines = (buffer + data).split("\n");
		buffer = lines.pop() ?? "";
		if (reportAntivirusBlocks(lines.join("\n"), options.instance) && !blocked) {
			blocked = true;
			options.onBlocked?.();
		}
	});
}
