// Tray integration. The TrayIcon and Menu live in the Electron main process;
// this module only pushes localized, declarative state to it (labels and
// dynamic entries) and reacts to the host's navigation events. All actual
// tray behaviour — showing the window, quitting, left-click handling — is
// host-side.
import { goto } from "$app/navigation";
import { lobbyManager } from "$lib/lobby/lobby-manager";
import { lobbyHost } from "$lib/lobby/stores.svelte";
import { m } from "$lib/paraglide/messages";
import { invoke, listen } from "$lib/platform/api";
import { queueItems, queueRunning } from "$lib/rigby/stores.svelte";
import { preparedInstances } from "$lib/stores/instance.svelte";
import { localeState } from "$lib/stores/locale.svelte";
import { lobbyServerProcessesList, processesList } from "$lib/stores/processes.svelte";
import type { TrayEntry, TrayNavId } from "../../../electron/src/ipc/contract";

/**
 * Whether closing the window should keep the launcher alive in the system
 * tray. Extend this predicate with more conditions as needed.
 * @returns {boolean} true when background work requires the app to stay alive
 */
export function shouldKeepRunningInTray(): boolean {
	return (
		processesList.value.length > 0 ||
		lobbyServerProcessesList.value.length > 0 ||
		queueRunning.value ||
		queueItems.value.some((item) => item.status === "running") ||
		lobbyManager.isConnected()
	);
}

const separator: TrayEntry = { kind: "separator" };

function navEntry(path: string, text: string): TrayEntry {
	return { kind: "item", id: `nav:${path}` as TrayNavId, text, enabled: true };
}

/**
 * Tray menu entries mirroring the sidebar pages, plus Open/Quit.
 * @returns {TrayEntry[]} the current menu entries
 */
function trayEntries(): TrayEntry[] {
	const entries: TrayEntry[] = [
		{ kind: "item", id: "open", text: m.tray_open(), enabled: true },
		separator,
		navEntry("/", m.sidebar_home()),
		navEntry("/library", m.sidebar_library()),
		navEntry("/downloads", m.sidebar_downloads()),
		navEntry("/servers", m.sidebar_servers()),
	];

	if (lobbyHost.value) {
		entries.push(navEntry("/lobby", m.sidebar_lobby()));
	}

	if (preparedInstances.value.length > 0) {
		entries.push(separator);
		for (const instance of preparedInstances.value) {
			entries.push(navEntry(`/instance/${instance.id}`, instance.label));
		}
	}

	if (lobbyServerProcessesList.value.length > 0) {
		entries.push(separator);
		for (const server of lobbyServerProcessesList.value) {
			entries.push(navEntry(`/lobby-admin/${server.child.pid}`, server.createOptions.name));
		}
	}

	entries.push(
		separator,
		navEntry("/logs", m.sidebar_logs()),
		navEntry("/settings", m.sidebar_settings()),
		separator,
		{ kind: "item", id: "quit", text: m.tray_quit(), enabled: true },
	);

	return entries;
}

$effect.root(() => {
	// Create the host-side tray once (upserted host-side, so dev hot reloads
	// reuse the same icon instead of stacking duplicates on Linux).
	void invoke("tray:ensure", { tooltip: "Tempest Launcher" });

	let lastKey = "";
	$effect(() => {
		void localeState.current; // rebuild localized labels on locale change
		const entries = trayEntries();
		const key = JSON.stringify(
			entries.map((e) => (e.kind === "separator" ? "-" : [e.id, e.text])),
		);
		if (key === lastKey) return;
		lastKey = key;
		void invoke("tray:set-state", { entries });
	});

	// Close guard: hide the window into the tray while background work (running
	// games, hosted servers, an active lobby) needs the launcher to stay alive.
	// The decision is rendered host-side by the main process close handler.
	$effect(() => {
		void invoke("app:set-close-behavior", { hideToTray: shouldKeepRunningInTray() });
	});

	// Tray menu nav items are handled host-side up to "show the window"; the
	// SPA route change comes back as an event.
	void listen("tray:navigate", ({ route }) => {
		void goto(route);
	});
});
