import { SvelteSet } from "svelte/reactivity";
import { invoke } from "$lib/platform/api";
import { getQueryClient } from "$lib/queries/client";

// Mods being downloaded or installed, shown as placeholder rows in the instance's mod list
// until the real entry shows up.
export type PendingInstall = {
	id: string;
	gamePath: string;
	/** Set when the mod may already be installed (repository installs): its row shows the
	 * install in place instead of a placeholder row. */
	modId?: string;
	name: string;
	version?: string;
	author?: string;
	/** Download progress 0–1; null while installing or when the size is unknown. */
	progress: number | null;
};

export const pendingInstalls = $state<{ value: PendingInstall[] }>({ value: [] });

/** Ids of mods whose install just finished, so their row fades in. */
export const justInstalled = new SvelteSet<string>();

// `downloading` starts the row at 0% instead of in the installing state.
export function trackInstall(
	gamePath: string,
	info: { name: string; version?: string; modId?: string },
	downloading = false,
) {
	const id = crypto.randomUUID();
	const progress = downloading ? 0 : null;
	pendingInstalls.value = [...pendingInstalls.value, { id, gamePath, ...info, progress }];
	const patch = (changes: Partial<PendingInstall>) => {
		pendingInstalls.value = pendingInstalls.value.map((p) =>
			p.id === id ? { ...p, ...changes } : p,
		);
	};
	// Empty values never overwrite what the row already shows.
	const setInfo = (info: { name?: string; version?: string; author?: string }) =>
		patch(Object.fromEntries(Object.entries(info).filter(([, v]) => v)));
	return {
		setProgress: (progress: number | null) => patch({ progress }),
		setInfo,
		// Lets the bar ease to full before it gives way to the installing spinner; the install
		// itself does not wait for this.
		downloadDone() {
			patch({ progress: 1 });
			setTimeout(() => patch({ progress: null }), 350);
		},
		// Fills in name, version and authors from the package's manifest once it is on disk.
		async readPackage(file: string) {
			const manifest = await invoke("mods:read-manifest", { path: file }).catch(() => null);
			if (!manifest) return;
			setInfo({
				name: manifest.name,
				version: manifest.version,
				author: manifest.authors.join(", "),
			});
		},
		// After a successful install, waits for the refreshed list so the real row replaces the
		// placeholder without a gap, and marks that row (`modId`) to fade in.
		async finish(installed: boolean, modId?: string) {
			if (installed) {
				await getQueryClient()?.invalidateQueries({ queryKey: ["mods", gamePath] });
				if (modId) {
					justInstalled.add(modId);
					setTimeout(() => justInstalled.delete(modId), 1000);
				}
			}
			pendingInstalls.value = pendingInstalls.value.filter((p) => p.id !== id);
		},
	};
}

export type InstallTracker = ReturnType<typeof trackInstall>;
