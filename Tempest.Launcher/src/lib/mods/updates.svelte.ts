import { listMods } from "$lib/core/mods";
import { installQueue, isGameRunning } from "$lib/mods/installQueue.svelte";
import { installFromRepo } from "$lib/mods/repoInstall";
import { getQueryClient } from "$lib/queries/client";
import { checkRepoUpdates, repoUpdatesKey, updatableMods } from "$lib/queries/repo";
import { instanceMap } from "$lib/stores/instance.svelte";
import { persistedJSON } from "$lib/stores/persisted.svelte";
import { autoInstallModUpdates } from "$lib/stores/settings.svelte";
import { updaterStore } from "$lib/stores/updater.svelte";
import type { ModRecord } from "$lib/core/mods";

// Quick window switches reuse the last answer instead of asking the repository again.
const RECHECK_MS = 60_000;

// A version the user went back to on purpose: no update prompts or installs over it.
const heldVersions = persistedJSON<Record<string, string>>("tempest_held_mod_versions", {});
const heldKey = (gamePath: string, modId: string) => `${gamePath}|${modId}`;

export function holdModVersion(gamePath: string, modId: string, version: string) {
	heldVersions.value = { ...heldVersions.value, [heldKey(gamePath, modId)]: version };
}

const isHeld = (gamePath: string, mod: ModRecord) =>
	heldVersions.value[heldKey(gamePath, mod.Id)] === mod.Version;

export const queuedUpdateOf = (gamePath: string, modId: string, version: string) =>
	installQueue.value.find(
		(q) => q.gamePath === gamePath && q.modId === modId && q.version === version,
	);

/** The instance whose mod updates the popup shows; ModUpdatesPrompt renders it. */
export const modUpdatePrompt = $state<{ gamePath: string | null }>({ gamePath: null });
let waitingPrompts: string[] = [];

export function openModUpdates(gamePath: string) {
	modUpdatePrompt.gamePath = gamePath;
}

export function closeModUpdates() {
	modUpdatePrompt.gamePath = waitingPrompts.shift() ?? null;
}

function showPrompts(gamePaths: string[]) {
	const current = modUpdatePrompt.gamePath;
	if (current) {
		waitingPrompts = gamePaths.filter((path) => path !== current);
		return;
	}
	// The popup waits behind the launcher's own update dialog, but never covers another
	// dialog the user is in the middle of; it comes back the next time the launcher gets focus.
	if (!updaterStore.isOpen && document.querySelector('[role="dialog"]')) return;
	waitingPrompts = gamePaths.slice(1);
	modUpdatePrompt.gamePath = gamePaths[0] ?? null;
}

// Each update is tried once per session; one that failed or was declined stays in the popup.
const attempted = new Set<string>();

// Installs what it may, and says whether the instance has updates left for the user: ones not
// installed yet, and ones downloaded that could install and reload right now.
async function checkInstance(gamePath: string, patch: string): Promise<boolean> {
	const client = getQueryClient();
	if (!client) return false;
	const mods = await client.fetchQuery({
		queryKey: ["mods", gamePath],
		queryFn: () => listMods(gamePath),
		staleTime: RECHECK_MS,
	});
	const installed = updatableMods(mods);
	if (installed.length === 0) return false;
	const updates = await client.fetchQuery({
		queryKey: repoUpdatesKey(patch, installed),
		queryFn: () => checkRepoUpdates(patch, installed),
		staleTime: RECHECK_MS,
	});
	const pending = updates.flatMap((update) => {
		const mod = mods.find((m) => m.Id === update.id);
		return mod &&
			mod.Enabled &&
			update.updateAvailable &&
			update.latest &&
			!isHeld(gamePath, mod)
			? [{ mod, version: update.latest.version }]
			: [];
	});

	const updated = new Set<string>();
	if (autoInstallModUpdates.value) {
		for (const { mod, version } of pending) {
			const key = `${heldKey(gamePath, mod.Id)}@${version}`;
			if (attempted.has(key) || queuedUpdateOf(gamePath, mod.Id, version)) continue;
			attempted.add(key);
			const outcome = await installFromRepo(gamePath, mod.Id, version, mod.Name, {
				replace: true,
				dependencies: "auto",
				waitForGame: true,
			});
			if (outcome.status === "installed") updated.add(mod.Id);
		}
	}
	return pending.some(({ mod, version }) => {
		if (updated.has(mod.Id)) return false;
		const queued = queuedUpdateOf(gamePath, mod.Id, version);
		return queued ? !!queued.hotReloadable && isGameRunning(gamePath) : true;
	});
}

let checking = false;

/** Looks for mod updates on every ready instance, installs them when that is on, then prompts. */
export async function checkModUpdates(): Promise<void> {
	if (checking) return;
	checking = true;
	try {
		const prompts: string[] = [];
		for (const instance of Object.values(instanceMap.value)) {
			if (!instance?.path || !instance.version || instance.state.type !== "prepared") {
				continue;
			}
			try {
				if (await checkInstance(instance.path, instance.version)) {
					prompts.push(instance.path);
				}
			} catch (error) {
				console.warn(`mod update check failed for ${instance.label}`, error);
			}
		}
		showPrompts(prompts);
	} finally {
		checking = false;
	}
}
