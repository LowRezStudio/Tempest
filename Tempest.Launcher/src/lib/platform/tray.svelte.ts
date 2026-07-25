import { goto } from "$app/navigation";
import { Menu, type MenuItemOptions, type PredefinedMenuItemOptions } from "@tauri-apps/api/menu";
import { TrayIcon, type TrayIconEvent } from "@tauri-apps/api/tray";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { lobbyManager } from "$lib/lobby/lobby-manager";
import { lobbyHost } from "$lib/lobby/stores.svelte";
import { m } from "$lib/paraglide/messages";
import { queueItems, queueRunning } from "$lib/rigby/stores.svelte";
import { preparedInstances } from "$lib/stores/instance.svelte";
import { localeState } from "$lib/stores/locale.svelte";
import { lobbyServerProcessesList, processesList } from "$lib/stores/processes.svelte";

const TRAY_ID = "main";

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

async function showMainWindow(): Promise<void> {
	const window = getCurrentWindow();
	await window.unminimize();
	await window.show();
	await window.setFocus();
}

async function quitApp(): Promise<void> {
	// destroy() skips the close-requested guard below, so this always quits.
	await getCurrentWindow().destroy();
}

function handleTrayIconEvent(event: TrayIconEvent): void {
	if (event.type === "Click" && event.button === "Left" && event.buttonState === "Up") {
		void showMainWindow();
	}
}

async function handleTrayAction(id: string): Promise<void> {
	if (id === "quit") {
		await quitApp();
	} else if (id === "open") {
		await showMainWindow();
	} else if (id.startsWith("nav:")) {
		await showMainWindow();
		await goto(id.slice(4));
	}
}

type TrayEntry = MenuItemOptions | PredefinedMenuItemOptions;

const separator: PredefinedMenuItemOptions = { item: "Separator" };

function navEntry(path: string, text: string): MenuItemOptions {
	return { id: `nav:${path}`, text };
}

/**
 * Tray menu entries mirroring the sidebar pages, plus Open/Quit.
 * @returns {TrayEntry[]} the current menu entries
 */
function trayEntries(): TrayEntry[] {
	const entries: TrayEntry[] = [
		{ id: "open", text: m.tray_open() },
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
		{ id: "quit", text: m.tray_quit() },
	);

	return entries;
}

function withActions(entries: TrayEntry[]): TrayEntry[] {
	return entries.map((entry) =>
		"item" in entry ? entry : { ...entry, action: (id: string) => void handleTrayAction(id) },
	);
}

// Replace any tray left over from a previous renderer session (dev reloads),
// then create ours. The tray outlives the window so the app can hide into it.
const trayPromise: Promise<TrayIcon> = (async () => {
	await TrayIcon.removeById(TRAY_ID).catch(() => {});
	return TrayIcon.new({
		id: TRAY_ID,
		tooltip: "Tempest Launcher",
		showMenuOnLeftClick: false,
		action: handleTrayIconEvent,
	});
})();

$effect.root(() => {
	let lastKey = "";
	$effect(() => {
		void localeState.current; // rebuild localized labels on locale change
		const entries = trayEntries();
		const key = JSON.stringify(entries.map((e) => ("item" in e ? "-" : [e.id, e.text])));
		if (key === lastKey) return;
		lastKey = key;
		void trayPromise.then(async (tray) => {
			await tray.setMenu(await Menu.new({ items: withActions(entries) }));
		});
	});
});

// Close guard: hide the window into the tray while background work (running
// games, hosted servers, an active lobby) needs the launcher to stay alive.
$effect.root(() => {
	const appWindow = getCurrentWindow();
	let unlisten: (() => void) | undefined;
	void appWindow
		.onCloseRequested(async (event) => {
			event.preventDefault();
			await (shouldKeepRunningInTray() ? appWindow.hide() : appWindow.destroy());
		})
		.then((fn) => {
			unlisten = fn;
		});
	return () => unlisten?.();
});
