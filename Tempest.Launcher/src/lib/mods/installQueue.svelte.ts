import { installModFile } from "$lib/mods/installFile";
import { m } from "$lib/paraglide/messages";
import { invoke } from "$lib/platform/api";
import { instanceMap } from "$lib/stores/instance.svelte";
import { persistedJSON } from "$lib/stores/persisted.svelte";
import { processesList } from "$lib/stores/processes.svelte";
import { addToast } from "$lib/stores/ui.svelte";

// Installs the CLI refused because the game holds files they would replace (its DLLs, open
// packages). Instead of failing they wait here, with the choices already made (replace or
// stack, untrusted signer accepted), and run once the instance's game has closed. The package
// is copied aside first, so what was added is what installs. Kept across launcher restarts.
export type QueuedInstall = {
	id: string;
	gamePath: string;
	/** The queue's own copy of the package (see `mods:queue-stage`). */
	file: string;
	name: string;
	version?: string;
	author?: string;
	/** The package's manifest id, when it has one: a newer queued copy of the mod replaces it. */
	modId?: string;
	replace: boolean;
	stack: boolean;
	allowUntrusted: boolean;
	/** What was in use at the last attempt, relative to the game folder. */
	lockedFiles: string[];
	queuedAt: number;
};

/** The choices an install sequence made before it was refused; a queued run starts with them. */
export type InstallChoices = { replace: boolean; stack: boolean; allowUntrusted: boolean };

const stored = persistedJSON<QueuedInstall[]>("tempest_install_queue", []);

export const installQueue = {
	get value() {
		return stored.value;
	},
};

/** Entries running right now: their mod shows as an installing row in the list instead. */
export const runningQueued = $state<{ ids: string[] }>({ ids: [] });

export const isGameRunning = (gamePath: string) =>
	processesList.value.some((p) => p.instance?.path === gamePath);

const discard = (file: string) => invoke("mods:queue-discard", { path: file }).catch(() => {});

function remove(id: string) {
	const item = stored.value.find((q) => q.id === id);
	if (!item) return;
	stored.value = stored.value.filter((q) => q.id !== id);
	void discard(item.file);
}

// Queues a refused install, or updates the entry it came from (`fromQueue`). A newer copy of a
// mod already waiting replaces that entry. Throws when the package cannot be copied aside.
export async function enqueueInstall(
	info: Omit<QueuedInstall, "id" | "queuedAt">,
	fromQueue?: string,
): Promise<QueuedInstall> {
	const existing = fromQueue ? stored.value.find((q) => q.id === fromQueue) : undefined;
	if (existing) {
		const updated = { ...existing, lockedFiles: info.lockedFiles };
		stored.value = stored.value.map((q) => (q.id === existing.id ? updated : q));
		return updated;
	}
	const file = await invoke("mods:queue-stage", { path: info.file });
	const item: QueuedInstall = { ...info, file, id: crypto.randomUUID(), queuedAt: Date.now() };
	const superseded = stored.value.filter(
		(q) =>
			q.gamePath === info.gamePath &&
			!runningQueued.ids.includes(q.id) &&
			(info.modId ? q.modId === info.modId : q.name === info.name),
	);
	for (const old of superseded) void discard(old.file);
	stored.value = [...stored.value.filter((q) => !superseded.includes(q)), item];
	return item;
}

// Drops a waiting entry without installing it.
export function removeQueuedInstall(id: string) {
	if (runningQueued.ids.includes(id)) return;
	remove(id);
}

// Runs one entry through the regular install sequence, starting from the stored choices.
// Leaves it queued when its files are still in use; any other outcome takes it off the queue.
async function runQueued(item: QueuedInstall, manual: boolean): Promise<void> {
	if (runningQueued.ids.includes(item.id)) return;
	runningQueued.ids = [...runningQueued.ids, item.id];
	try {
		const outcome = await installModFile(item.gamePath, item.file, item.name, {
			replace: item.replace,
			stack: item.stack,
			allowUntrusted: item.allowUntrusted,
			dependencies: "skip",
			queued: item,
			force: manual,
		});
		if (outcome.status === "queued") {
			if (manual) {
				addToast({
					title: m.install_queue_still_in_use_title(),
					message: m.install_queue_still_in_use_message({ name: item.name }),
					tone: "warning",
				});
			}
			return;
		}
		remove(item.id);
		if (outcome.status === "installed") {
			addToast({
				title: m.toast_mod_installed_title(),
				message: m.toast_mod_installed_message({
					name: outcome.result.Mod?.Name ?? item.name,
				}),
				tone: "success",
			});
		}
	} finally {
		runningQueued.ids = runningQueued.ids.filter((id) => id !== item.id);
	}
}

// "Install now": tries one entry right away, game running or not.
export function installQueuedNow(id: string): Promise<void> {
	const item = stored.value.find((q) => q.id === id);
	return item ? runQueued(item, true) : Promise.resolve();
}

const draining = new Set<string>();

// Works through an instance's queue in the order it was added, stopping when its game starts
// again; entries whose files are still in use stay for the next time it closes.
async function drain(gamePath: string): Promise<void> {
	if (draining.has(gamePath)) return;
	draining.add(gamePath);
	try {
		const instanceExists = Object.values(instanceMap.value).some((i) => i?.path === gamePath);
		for (const item of stored.value.filter((q) => q.gamePath === gamePath)) {
			if (!instanceExists) {
				remove(item.id);
				continue;
			}
			if (isGameRunning(gamePath)) break;
			// Removed or replaced by a newer copy meanwhile.
			if (!stored.value.some((q) => q.id === item.id)) continue;
			await runQueued(item, false);
		}
	} finally {
		draining.delete(gamePath);
	}
}

// The game only lets go of its files once its process is gone; a moment more for the OS.
const SETTLE_MS = 1500;

let started = false;

/**
 * Starts processing the queue: whenever an instance's game closes, and once now for instances
 * whose game is not running (entries left from an earlier session). Call after the running
 * games have been restored, so a game that survived a renderer reload is not mistaken for
 * closed.
 */
export function startInstallQueue() {
	if (started) return;
	started = true;
	void invoke("mods:queue-prune", { keep: stored.value.map((q) => q.file) }).catch(() => {});
	$effect.root(() => {
		let running = new Set<string>();
		$effect(() => {
			const now = new Set(
				processesList.value.flatMap((p) => (p.instance?.path ? [p.instance.path] : [])),
			);
			for (const gamePath of running) {
				if (!now.has(gamePath)) setTimeout(() => void drain(gamePath), SETTLE_MS);
			}
			running = now;
		});
	});
	const waiting = new Set(stored.value.map((q) => q.gamePath));
	for (const gamePath of waiting) {
		if (!isGameRunning(gamePath)) void drain(gamePath);
	}
}
