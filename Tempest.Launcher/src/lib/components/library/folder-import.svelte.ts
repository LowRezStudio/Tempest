import { useQueryClient } from "@tanstack/svelte-query";
import { untrack } from "svelte";
import { listMods, type ModRecord } from "$lib/core/mods";
import versions from "$lib/data/versions.json";
import { m } from "$lib/paraglide/messages";
import { buildQueryOptions } from "$lib/queries/core";
import { createFoundInstallationsQuery, createSetupInstanceMutation } from "$lib/queries/instance";
import { folderName } from "$lib/rigby/format";
import { addInstance, instanceMap, updateInstance } from "$lib/stores/instance.svelte";
import { defaultInstancePath } from "$lib/stores/settings.svelte";
import type { InstallationSource } from "$lib/platforms/discovery";
import type { Instance } from "$lib/types/instance";
import type { Patch } from "$lib/utils/versions";

export type LoginMethod = "steam" | "epic" | "hirez";

type Detection = {
	versionId: string;
	/** The folder can't be imported (not a game install, or the CLI failed). */
	error: string;
	/** A game install whose exact build is unknown; the user picks the patch. */
	warning: string;
};

export type Folder = Detection & {
	/** The normalized path, so one folder spelled two ways is listed once */
	key: string;
	path: string;
	/** Where the scan found it; unset for folders added by hand */
	source?: InstallationSource;
	selected: boolean;
	loading: boolean;
	mods: ModRecord[];
	modsLoading: boolean;
};

const normalizePath = (p: string) =>
	p
		.trim()
		.replace(/[\\/]+$/, "")
		.toLowerCase();

export function findExistingInstance(targetPath: string) {
	const target = normalizePath(targetPath);
	return Object.values(instanceMap.value).find(
		(i) => !!i?.path && normalizePath(i.path) === target,
	);
}

// Another copy gets its own name too, and with it its own color.
export function freeLabel(label: string) {
	const taken = new Set(Object.values(instanceMap.value).map((i) => i?.label));
	let candidate = label;
	for (let n = 2; taken.has(candidate); n++) candidate = `${label} (${n})`;
	return candidate;
}

export function loginOf(source: InstallationSource | undefined): LoginMethod | undefined {
	return source === "steam" || source === "epic" ? source : undefined;
}

export function argsFor(version: string | undefined, method: LoginMethod | undefined) {
	if (version !== "8.1") return [];
	return method === "steam" ? ["-steam"] : method === "epic" ? ["-epic"] : [];
}

export function newInstance(
	fields: Pick<Instance, "label" | "version" | "manifestId" | "appId" | "path" | "state">,
	args: string[],
): Instance {
	return {
		id: crypto.randomUUID(),
		...fields,
		launchOptions: { dllList: [], args, noDefaultArgs: false, log: false },
	};
}

/**
 * Importing game folders as instances: what the scan found and folders added by hand, each
 * identified, ticked ones imported. Create it while a component initializes.
 */
export class FolderImport {
	folders = $state<Folder[]>([]);
	/** The patch picked for a folder whose build wasn't recognised, by folder key */
	picked = $state<Record<string, string>>({});
	loginMethod = $state<LoginMethod>();

	readonly found: ReturnType<typeof createFoundInstallationsQuery>;

	/** Ticked folders that can become instances */
	chosen = $derived(
		this.folders.filter((f) => f.selected && !f.error && !findExistingInstance(f.path)),
	);
	single = $derived(this.chosen.length === 1 ? this.chosen[0] : undefined);
	bulk = $derived(this.chosen.length > 1);
	detecting = $derived(this.chosen.some((f) => f.loading));
	needsPatch = $derived(!!this.single && !this.single.loading && !!this.single.warning);
	patch = $derived(this.single ? this.patchOf(this.single) : undefined);
	needsLogin = $derived(this.patch?.version === "8.1");
	ready = $derived(
		this.chosen.length > 0 &&
			!this.detecting &&
			(this.bulk || (!!this.patch && (!this.needsLogin || !!this.loginMethod))),
	);
	// Folders already in the library sink to the bottom, the rest keep the order they came in.
	listed = $derived(
		[...this.folders].sort(
			(a, b) => +!!findExistingInstance(a.path) - +!!findExistingInstance(b.path),
		),
	);

	readonly #name: () => string;
	readonly #queryClient = useQueryClient();
	readonly #setup = createSetupInstanceMutation();

	/**
	 * @param active - Whether to scan and list what it finds.
	 * @param name - The name the user gave the new instances, if any.
	 */
	constructor(active: () => boolean, name: () => string = () => "") {
		this.#name = name;
		this.found = createFoundInstallationsQuery(
			() => (defaultInstancePath.value ? [defaultInstancePath.value] : []),
			active,
		);

		// Scan results join the list while it is shown; they wait unticked.
		$effect(() => {
			const found = this.found.data;
			if (!active() || !found) return;
			untrack(() => this.add(found, false));
		});

		// An 8.1 copy found through Steam or Epic signs in there.
		$effect(() => {
			const suggested = this.needsLogin ? loginOf(this.single?.source) : undefined;
			if (suggested) this.loginMethod = suggested;
		});
	}

	// The patch an imported folder gets: what it was identified as, or the pick for an unknown build.
	patchOf(folder: Folder): Patch | undefined {
		const id = this.picked[folder.key] ?? folder.versionId;
		return versions.find((v) => v.id === id);
	}

	label(folder: Folder, found: Patch | undefined, several = this.bulk) {
		const name = this.#name().trim();
		const dir = folderName(folder.path);
		if (several) {
			return name ? `${name} - ${dir}` : freeLabel(found?.name ?? dir);
		}
		return name || freeLabel(found?.name ?? m.wizard_paladins_instance());
	}

	/**
	 * Lists folders and identifies the new ones; one already listed is only ticked.
	 * @param entries - The folders, with where the scan found them.
	 * @param selected - Whether they start ticked: folders added by hand do, scan results don't.
	 */
	add(entries: { path: string; source?: InstallationSource }[], selected = true) {
		const added: Folder[] = [];
		for (const { path, source } of entries) {
			const key = normalizePath(path);
			const listed =
				this.folders.find((f) => f.key === key) ?? added.find((f) => f.key === key);
			if (listed) {
				if (selected) listed.selected = true;
				continue;
			}
			// A folder already in the library is only listed, under the patch it was set up with.
			const instance = findExistingInstance(path);
			added.push({
				key,
				path,
				source,
				selected,
				versionId: instance
					? (instance.manifestId ??
						versions.find((v) => v.version === instance.version)?.id ??
						"")
					: "",
				error: "",
				warning: "",
				loading: !instance,
				mods: [],
				modsLoading: !instance,
			});
		}
		if (added.length === 0) return;
		this.folders = [...this.folders, ...added];
		void this.#detectFolders(added.filter((f) => f.loading).map((f) => f.key));
	}

	remove(key: string) {
		this.folders = this.folders.filter((f) => f.key !== key);
		delete this.picked[key];
	}

	/**
	 * Adds the ticked folders to the library and sets each up.
	 * @returns The new instances.
	 */
	import(): Instance[] {
		// Each instance added leaves `chosen`, so take the list as it was clicked.
		const picks = [...this.chosen];
		const several = picks.length > 1;
		return picks.map((folder) => {
			const found = this.patchOf(folder);
			// With several at once there is no sign-in question; each goes by where it was found.
			const method = several ? loginOf(folder.source) : this.loginMethod;
			const instance = newInstance(
				{
					label: this.label(folder, found, several),
					version: found?.version,
					manifestId: found?.id,
					appId: found?.appId ?? 444090,
					path: folder.path,
					state: { type: "setup" },
				},
				argsFor(found?.version, method),
			);
			addInstance(instance);
			void this.#runSetup(instance);
			return instance;
		});
	}

	reset() {
		this.folders = [];
		this.picked = {};
		this.loginMethod = undefined;
	}

	async #runSetup(instance: Instance) {
		try {
			await this.#setup.mutateAsync(instance);
		} catch (error) {
			console.error("Instance setup failed:", error);
		} finally {
			updateInstance(instance.id, { state: { type: "prepared" } });
		}
	}

	async #detect(path: string): Promise<Detection> {
		try {
			const info = await this.#queryClient.fetchQuery(buildQueryOptions(path));
			if (!info) return { versionId: "", error: m.wizard_could_not_identify(), warning: "" };
			const exact = versions.find((v) => v.id === info.Id);
			const version =
				exact ??
				versions.findLast((v) => !!info.VersionGroup && v.version === info.VersionGroup);
			return {
				versionId: version?.id ?? "",
				error: "",
				warning: exact
					? ""
					: info.Id
						? m.wizard_build_not_in_database({
								patchName: info.PatchName,
								versionGroup: info.VersionGroup,
							})
						: m.wizard_unknown_build(),
			};
		} catch (error) {
			console.error("Detection error:", error);
			return { versionId: "", error: m.wizard_identify_error(), warning: "" };
		}
	}

	// By key: a folder removed meanwhile is simply not updated.
	#updateFolder(key: string, fields: Partial<Folder>) {
		const folder = this.folders.find((f) => f.key === key);
		if (folder) Object.assign(folder, fields);
	}

	async #detectFolders(keys: string[]) {
		for (const key of keys) {
			const path = this.folders.find((f) => f.key === key)?.path;
			if (!path) continue;
			const detection = await this.#detect(path);
			this.#updateFolder(key, { ...detection, loading: false });
			const mods = detection.error ? [] : await listMods(path).catch(() => []);
			this.#updateFolder(key, { mods, modsLoading: false });
		}
	}
}
