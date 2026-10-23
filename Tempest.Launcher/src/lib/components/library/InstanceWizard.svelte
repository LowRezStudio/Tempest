<script lang="ts">
	import { goto } from "$app/navigation";
	import {
		Check,
		ChevronDown,
		CircleAlert,
		CloudDownload,
		Copy,
		ExternalLink,
		FolderOpen,
		FolderX,
		Gamepad2,
		Info,
		Plus,
		RotateCw,
		ShoppingBag,
		Terminal,
		UserRound,
		X,
	} from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { untrack } from "svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { listMods, type ModRecord } from "$lib/core/mods";
	import versions from "$lib/data/versions.json";
	import { m } from "$lib/paraglide/messages";
	import { open as openDialog, openUrl, join, platform } from "$lib/platform/api";
	import { buildQueryOptions } from "$lib/queries/core";
	import {
		createFoundInstallationsQuery,
		createSetupInstanceMutation,
	} from "$lib/queries/instance";
	import {
		RIGBY_BASE_URL,
		RIGBY_MANIFEST_URL_TEMPLATE,
		WIKI_BASE_URL,
	} from "$lib/rigby/constants";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { addInstance, instanceMap, updateInstance } from "$lib/stores/instance.svelte";
	import { defaultInstancePath } from "$lib/stores/settings.svelte";
	import { instanceWizardTab } from "$lib/stores/ui.svelte";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import PatchTimeline from "./PatchTimeline.svelte";
	import type { InstallationSource } from "$lib/platforms/discovery";
	import type { Instance } from "$lib/types/instance";
	import type { Patch } from "$lib/utils/versions";

	interface Props {
		open?: boolean;
	}

	let { open = $bindable(false) }: Props = $props();

	type Mode = "download" | "folder";
	type LoginMethod = "steam" | "epic" | "hirez";

	type Detection = {
		versionId: string;
		/** The folder can't be imported (not a game install, or the CLI failed). */
		error: string;
		/** A game install whose exact build is unknown; the user picks the patch. */
		warning: string;
	};
	type Folder = Detection & {
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

	const modes = [
		{
			mode: "download",
			Icon: CloudDownload,
			title: m.library_download_patch,
			hint: m.library_download_patch_hint,
		},
		{
			mode: "folder",
			Icon: FolderOpen,
			title: m.library_import_folder,
			hint: m.library_import_folder_hint,
		},
	] as const;

	const logins = [
		{
			method: "steam",
			Icon: Gamepad2,
			title: m.wizard_login_steam,
			hint: m.wizard_login_steam_desc,
		},
		{
			method: "epic",
			Icon: ShoppingBag,
			title: m.wizard_login_epic,
			hint: m.wizard_login_epic_desc,
		},
		{
			method: "hirez",
			Icon: UserRound,
			title: m.wizard_login_hirez,
			hint: m.wizard_login_hirez_desc,
		},
	] as const;

	const sources: Record<InstallationSource, { Icon: typeof X; label?: () => string }> = {
		steam: { Icon: Gamepad2, label: m.wizard_login_steam },
		epic: { Icon: ShoppingBag, label: m.wizard_login_epic },
		hirez: { Icon: UserRound, label: m.wizard_login_hirez },
		tempest: { Icon: CloudDownload, label: m.wizard_source_tempest },
		folder: { Icon: FolderOpen },
	};

	// 8.2 was never released, so there is nothing to download; a folder can still be one.
	const downloadable = versions.filter((v) => v.version !== "8.2");

	let mode = $state<Mode>("download");
	let name = $state("");
	let downloadId = $state("");
	let installFolder = $state("");
	let installTarget = $state<{ path: string; copy: boolean } | null>(null);
	let pathInput = $state("");
	let folders = $state<Folder[]>([]);
	/** The patch picked for a folder whose build wasn't recognised, by folder key */
	let picked = $state<Record<string, string>>({});
	let loginMethod = $state<LoginMethod>();
	let copyStatus = $state<"idle" | "copied" | "failed">("idle");

	const foundQuery = createFoundInstallationsQuery(
		() => (defaultInstancePath.value ? [defaultInstancePath.value] : []),
		() => open && mode === "folder",
	);

	/** Ticked folders that can become instances */
	const chosen = $derived(
		folders.filter((f) => f.selected && !f.error && !findExistingInstance(f.path)),
	);
	const single = $derived(chosen.length === 1 ? chosen[0] : undefined);
	const bulk = $derived(chosen.length > 1);
	const detecting = $derived(chosen.some((f) => f.loading));
	const needsImportPatch = $derived(!!single && !single.loading && !!single.warning);

	const downloadPatch = $derived(versions.find((v) => v.id === downloadId));
	const importPatch = $derived(single ? folderPatch(single) : undefined);
	const patch = $derived(mode === "download" ? downloadPatch : importPatch);

	const appId = $derived(patch?.appId ?? 444090);
	const isPts = $derived(
		(appId !== 444090 && appId !== 44090) || !!patch?.name?.toLowerCase().includes("pts"),
	);
	const depotId = $derived(isPts ? 596351 : 444091);

	const needsLogin = $derived(patch?.version === "8.1");
	const loginReady = $derived(!needsLogin || !!loginMethod);

	// An 8.1 copy found through Steam or Epic signs in there.
	const suggestedLogin = $derived(
		mode === "folder" && needsLogin ? loginOf(single?.source) : undefined,
	);
	$effect(() => {
		if (suggestedLogin) loginMethod = suggestedLogin;
	});

	const ready = $derived(
		mode === "download"
			? !!downloadPatch && loginReady
			: chosen.length > 0 && !detecting && (bulk || (!!importPatch && loginReady)),
	);

	const depotDownloaderCommand = $derived.by(() => {
		if (!downloadPatch) return "";
		const binary = platform() === "windows" ? ".\\DepotDownloader.exe" : "./DepotDownloader";
		const dir = installTarget?.path || "<install-path>";
		return `${binary} -app ${appId} -depot ${depotId} -manifest ${downloadPatch.id} -os windows -dir "${dir}" -qr -remember-password`;
	});

	// A patch already in the library gets another copy beside it: "8.1 (2)", then "8.1 (3)".
	async function resolveTarget(version: string) {
		const base = installFolder.trim() || defaultInstancePath.value;
		for (let n = 1; ; n++) {
			const folder = n === 1 ? version : `${version} (${n})`;
			const path = base ? await join(base, folder) : `/instances/${folder}`;
			if (!findExistingInstance(path)) return { path, copy: n > 1 };
		}
	}

	// Another copy gets its own name too, and with it its own color.
	function freeLabel(label: string) {
		const taken = new Set(Object.values(instanceMap.value).map((i) => i?.label));
		let candidate = label;
		for (let n = 2; taken.has(candidate); n++) candidate = `${label} (${n})`;
		return candidate;
	}

	$effect(() => {
		const version = downloadPatch?.version;
		if (!version) {
			installTarget = null;
			return;
		}
		let stale = false;
		void resolveTarget(version).then((target) => {
			if (!stale) installTarget = target;
		});
		return () => {
			stale = true;
		};
	});

	const normalizePath = (p: string) =>
		p
			.trim()
			.replace(/[\\/]+$/, "")
			.toLowerCase();

	function findExistingInstance(targetPath: string) {
		const target = normalizePath(targetPath);
		return Object.values(instanceMap.value).find(
			(i) => !!i?.path && normalizePath(i.path) === target,
		);
	}

	function loginOf(source: InstallationSource | undefined): LoginMethod | undefined {
		return source === "steam" || source === "epic" ? source : undefined;
	}

	function argsFor(version: string | undefined, method: LoginMethod | undefined) {
		if (version !== "8.1") return [];
		return method === "steam" ? ["-steam"] : method === "epic" ? ["-epic"] : [];
	}

	async function browseInstallFolder() {
		const path = await openDialog({
			directory: true,
			title: m.wizard_select_install_folder(),
			defaultPath: installFolder || defaultInstancePath.value || undefined,
		});
		if (path) installFolder = path;
	}

	async function browseGameFolders() {
		const result = await openDialog({
			directory: true,
			multiple: true,
			title: m.wizard_select_installation_folder(),
			defaultPath: defaultInstancePath.value || undefined,
		});
		addFolders((result ?? []).filter(Boolean).map((path) => ({ path })));
	}

	function commitPathInput() {
		const path = pathInput.trim();
		if (path) addFolders([{ path }]);
		pathInput = "";
	}

	function removeFolder(key: string) {
		folders = folders.filter((f) => f.key !== key);
		delete picked[key];
	}

	/**
	 * Lists folders and identifies the new ones; one already listed is only ticked.
	 * @param entries - The folders, with where the scan found them.
	 * @param selected - Whether they start ticked: folders added by hand do, scan results don't.
	 */
	function addFolders(entries: { path: string; source?: InstallationSource }[], selected = true) {
		const added: Folder[] = [];
		for (const { path, source } of entries) {
			const key = normalizePath(path);
			const listed = folders.find((f) => f.key === key) ?? added.find((f) => f.key === key);
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
		folders = [...folders, ...added];
		void detectFolders(added.filter((f) => f.loading).map((f) => f.key));
	}

	// Scan results join the list once the import side is open; they wait unticked.
	$effect(() => {
		const found = foundQuery.data;
		if (!open || mode !== "folder" || !found) return;
		untrack(() => addFolders(found, false));
	});

	// Folders already in the library sink to the bottom, the rest keep the order they came in.
	const listed = $derived(
		[...folders].sort(
			(a, b) => +!!findExistingInstance(a.path) - +!!findExistingInstance(b.path),
		),
	);

	const queryClient = useQueryClient();

	async function detect(path: string): Promise<Detection> {
		try {
			const info = await queryClient.fetchQuery(buildQueryOptions(path));
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
	function updateFolder(key: string, fields: Partial<Folder>) {
		const folder = folders.find((f) => f.key === key);
		if (folder) Object.assign(folder, fields);
	}

	async function detectFolders(keys: string[]) {
		for (const key of keys) {
			const path = folders.find((f) => f.key === key)?.path;
			if (!path) continue;
			const detection = await detect(path);
			updateFolder(key, { ...detection, loading: false });
			const mods = detection.error ? [] : await listMods(path).catch(() => []);
			updateFolder(key, { mods, modsLoading: false });
		}
	}

	const setupInstanceMutation = createSetupInstanceMutation();

	const runSetup = async (instance: Instance) => {
		try {
			await setupInstanceMutation.mutateAsync(instance);
		} catch (error) {
			console.error("Instance setup failed:", error);
		} finally {
			updateInstance(instance.id, { state: { type: "prepared" } });
		}
	};

	// The patch an imported folder gets: what it was identified as, or the pick for an unknown build.
	function folderPatch(folder: Folder): Patch | undefined {
		const id = picked[folder.key] ?? folder.versionId;
		return versions.find((v) => v.id === id);
	}

	function folderLabel(folder: Folder, found: Patch | undefined, several = bulk) {
		const folderName = folder.path.split(/[\\/]/).pop() || folder.path;
		if (several) {
			return name.trim()
				? `${name.trim()} - ${folderName}`
				: freeLabel(found?.name || found?.version || folderName);
		}
		return (
			name.trim() || freeLabel(found?.name || found?.version || m.wizard_paladins_instance())
		);
	}

	function newInstance(
		fields: Pick<Instance, "label" | "version" | "manifestId" | "appId" | "path" | "state">,
		args: string[],
	): Instance {
		return {
			id: crypto.randomUUID(),
			...fields,
			launchOptions: { dllList: [], args, noDefaultArgs: false, log: false },
		};
	}

	async function handleCreate() {
		if (!ready) return;

		if (mode === "download") {
			const target = downloadPatch!;
			const { path } = await resolveTarget(target.version);
			addInstance(
				newInstance(
					{
						label: name.trim() || freeLabel(target.name || target.version),
						version: target.version,
						manifestId: target.id,
						appId,
						path,
						state: { type: "downloading" },
					},
					argsFor(target.version, loginMethod),
				),
			);
			restoreQueue.add({
				manifests: [RIGBY_MANIFEST_URL_TEMPLATE.replace("{id}", target.id)],
				outDir: path,
				baseUrl: RIGBY_BASE_URL,
			});
			open = false;
			void goto("/downloads");
			return;
		}

		// Each instance added leaves `chosen`, so take the list as it was clicked.
		const picks = [...chosen];
		const several = picks.length > 1;
		for (const folder of picks) {
			const found = folderPatch(folder);
			// With several at once there is no sign-in question; each goes by where it was found.
			const method = several ? loginOf(folder.source) : loginMethod;
			const instance = newInstance(
				{
					label: folderLabel(folder, found, several),
					version: found?.version,
					manifestId: found?.id,
					appId: found?.appId ?? 444090,
					path: folder.path,
					state: { type: "setup" },
				},
				argsFor(found?.version, method),
			);
			addInstance(instance);
			void runSetup(instance);
		}
		open = false;
	}

	$effect(() => {
		if (open) {
			mode = instanceWizardTab.value;
		} else {
			instanceWizardTab.value = "download";
			mode = "download";
			name = "";
			downloadId = "";
			installFolder = "";
			pathInput = "";
			folders = [];
			picked = {};
			loginMethod = undefined;
		}
	});

	async function handleCopyCommand() {
		if (!depotDownloaderCommand) return;
		try {
			await navigator.clipboard.writeText(depotDownloaderCommand);
			copyStatus = "copied";
		} catch (error) {
			console.error("Failed to copy command:", error);
			copyStatus = "failed";
		}
		setTimeout(() => {
			copyStatus = "idle";
		}, 2000);
	}

	async function openPatchNotes(target: Patch) {
		if (!target.wikiReference) return;
		try {
			await openUrl(
				`${WIKI_BASE_URL}${encodeURIComponent(target.wikiReference)}?fandom=allow`,
			);
		} catch (error) {
			console.error("Failed to open wiki:", error);
		}
	}
</script>

{#snippet patchHeading(patch: Patch | undefined)}
	<span class="text-base-content/60 min-w-0 truncate text-xs">
		{m.wizard_game_version()}
		{#if patch}
			<span class="text-base-content ml-1">
				<span class="font-mono font-semibold">{patch.version}</span> · {patch.name}
			</span>
		{/if}
	</span>
{/snippet}

<Modal
	bind:open
	title={m.wizard_title()}
	icon={Plus}
	description={m.wizard_description()}
	class="max-w-xl"
	onsubmit={handleCreate}
>
	<div class="flex flex-col gap-5">
		<div role="radiogroup" aria-label={m.wizard_mode_label()} class="grid grid-cols-2 gap-2">
			{#each modes as o (o.mode)}
				{@const picked = mode === o.mode}
				<button
					type="button"
					role="radio"
					aria-checked={picked}
					class={[
						"rounded-field flex cursor-pointer items-start gap-2.5 border p-3 text-left transition-colors",
						picked
							? "border-primary bg-primary/10"
							: "border-base-300 bg-base-100 hover:border-primary/50",
					]}
					onclick={() => (mode = o.mode)}
				>
					<o.Icon
						size={16}
						class={[
							"mt-0.5 shrink-0",
							picked ? "text-primary" : "text-base-content/50",
						]}
					/>
					<span class="min-w-0">
						<span class="block text-sm leading-tight font-medium">{o.title()}</span>
						<span class="text-base-content/60 mt-0.5 block text-xs leading-snug"
							>{o.hint()}</span
						>
					</span>
				</button>
			{/each}
		</div>

		{#if mode === "download"}
			<div class="flex flex-col gap-2">
				<div class="flex items-baseline justify-between gap-2">
					{@render patchHeading(downloadPatch)}
					{#if downloadPatch?.wikiReference}
						<button
							type="button"
							class="link flex items-center gap-1 text-xs"
							onclick={() => openPatchNotes(downloadPatch)}
						>
							{m.wizard_open_wiki()}
							<ExternalLink size={10} />
						</button>
					{/if}
				</div>
				<PatchTimeline
					patches={downloadable}
					bind:value={downloadId}
					label={m.wizard_game_version()}
				/>
			</div>
		{:else}
			<div class="flex flex-col gap-1">
				<div class="flex min-h-6 items-center justify-between gap-2">
					<span class="text-base-content/60 text-xs">{m.wizard_found_title()}</span>
					{#if foundQuery.isFetching}
						<span class="text-base-content/60 flex items-center gap-1.5 text-xs">
							<span class="loading loading-spinner loading-xs"></span>
							{m.wizard_scanning()}
						</span>
					{:else}
						<button
							type="button"
							class="btn btn-ghost btn-xs"
							onclick={() => foundQuery.refetch()}
						>
							<RotateCw size={12} />
							{m.wizard_scan_again()}
						</button>
					{/if}
				</div>

				{#if folders.length > 0}
					<ul
						class="bg-base-100 border-base-300 rounded-field divide-base-300 max-h-64 divide-y overflow-y-auto border"
					>
						{#each listed as folder (folder.key)}
							{@const existing = findExistingInstance(folder.path)}
							{@const found = folderPatch(folder)}
							{@const color = getInstanceColor(
								existing ?? { label: folderLabel(folder, found) },
							)}
							{@const contrast = getContrastColor(color)}
							{@const source = folder.source ? sources[folder.source] : undefined}
							{@const usable = !folder.error && !existing}
							<li>
								<label
									class={[
										"flex items-center gap-3 p-2.5 transition-colors",
										usable
											? "hover:bg-base-200/60 cursor-pointer"
											: "cursor-default",
										usable && folder.selected && "bg-primary/5",
										existing && "opacity-60",
									]}
								>
									<input
										type="checkbox"
										class="checkbox checkbox-sm checkbox-primary shrink-0"
										checked={usable && folder.selected}
										disabled={!usable}
										aria-label={folder.path}
										onchange={(e) =>
											(folder.selected = e.currentTarget.checked)}
									/>
									{#if folder.loading}
										<span
											class="bg-base-300 flex size-8 shrink-0 items-center justify-center rounded-full"
										>
											<span class="loading loading-spinner loading-xs"></span>
										</span>
									{:else if folder.error}
										<span
											class="bg-error/15 text-error flex size-8 shrink-0 items-center justify-center rounded-full"
										>
											<FolderX size={16} />
										</span>
									{:else if found}
										<!-- The sidebar's button the instance will get -->
										<span
											class="rounded-field flex size-8 shrink-0 items-center justify-center text-[10px] font-bold tabular-nums"
											style="background-color: {color}; color: {contrast};"
											>{found.version}</span
										>
									{:else}
										<span
											class="bg-warning/15 text-warning flex size-8 shrink-0 items-center justify-center rounded-full"
										>
											<CircleAlert size={16} />
										</span>
									{/if}
									<span class="min-w-0 flex-1">
										<span
											class="block truncate font-mono text-xs"
											title={folder.path}
										>
											{folder.path}
										</span>
										{#if folder.error}
											<span class="text-error block text-xs"
												>{folder.error}</span
											>
										{:else}
											{#if folder.warning && !folder.loading && !existing}
												<span class="text-warning block text-xs"
													>{folder.warning}</span
												>
											{/if}
											<span
												class="text-base-content/60 flex items-center gap-1 truncate text-xs"
											>
												{#if source?.label}
													<source.Icon size={12} class="shrink-0" />
													{source.label()} ·
												{/if}
												{#if existing}
													{m.wizard_already_in_library({
														name: existing.label,
													})}
												{:else if folder.loading}
													{m.common_identifying()}
												{:else}
													{#if found}
														<span class="text-base-content/80"
															>{found.name}</span
														>
														·
													{/if}
													{#if folder.modsLoading}
														{m.wizard_finding_mods()}
													{:else}
														<span
															title={folder.mods
																.map((mod) => mod.Name)
																.join(", ")}
														>
															{m.library_mods({
																count: folder.mods.length,
															})}
														</span>
													{/if}
												{/if}
											</span>
										{/if}
									</span>
									{#if !folder.source}
										<button
											type="button"
											class="btn btn-ghost btn-xs btn-square shrink-0"
											aria-label={m.wizard_remove_folder()}
											title={m.wizard_remove_folder()}
											onclick={() => removeFolder(folder.key)}
										>
											<X size={14} />
										</button>
									{/if}
								</label>
							</li>
						{/each}
					</ul>
				{:else if foundQuery.isFetching}
					<div
						class="border-base-300 rounded-field loading-shimmer text-base-content/60 border px-3 py-4 text-center text-xs"
					>
						{m.wizard_found_scanning()}
					</div>
				{:else if foundQuery.isError}
					<div
						class="bg-error/10 text-error rounded-field px-3 py-3 text-center text-xs"
						role="alert"
					>
						{m.wizard_found_failed()}
						<span class="mt-0.5 block font-mono wrap-break-word opacity-70"
							>{foundQuery.error.message}</span
						>
					</div>
				{:else}
					<div
						class="border-base-300 rounded-field text-base-content/60 border border-dashed px-3 py-4 text-center text-xs"
					>
						{m.wizard_found_none()}
					</div>
				{/if}
			</div>

			<div class="flex flex-col gap-1">
				<div class="join w-full">
					<input
						type="text"
						class="input input-sm join-item min-w-0 flex-1 font-mono"
						spellcheck="false"
						aria-label={m.wizard_game_folder()}
						placeholder={m.wizard_add_folder_placeholder()}
						bind:value={pathInput}
						onchange={commitPathInput}
						onkeydown={(e) => {
							// Enter adds the path instead of submitting the dialog.
							if (e.key === "Enter") {
								e.preventDefault();
								commitPathInput();
							}
						}}
					/>
					<button type="button" class="btn btn-sm join-item" onclick={browseGameFolders}>
						<FolderOpen size={14} />
						{m.common_browse()}
					</button>
				</div>
				<p class="text-base-content/60 text-xs">{m.wizard_game_folder_hint()}</p>
			</div>

			{#if needsImportPatch && single}
				<div class="flex flex-col gap-2">
					{@render patchHeading(importPatch)}
					<PatchTimeline
						patches={versions}
						bind:value={
							() => picked[single.key] ?? single.versionId,
							(id) => (picked[single!.key] = id)
						}
						label={m.wizard_game_version()}
					/>
				</div>
			{/if}
		{/if}

		{#if needsLogin}
			<fieldset>
				<legend class="mb-2 flex w-full items-baseline justify-between gap-2">
					<span class="text-sm font-medium">{m.wizard_login_prompt_title()}</span>
					<span class="text-base-content/50 text-xs">{m.wizard_login_prompt_desc()}</span>
				</legend>
				<div role="radiogroup" class="grid grid-cols-3 gap-2">
					{#each logins as o (o.method)}
						{@const picked = loginMethod === o.method}
						<button
							type="button"
							role="radio"
							aria-checked={picked}
							class={[
								"rounded-field flex cursor-pointer flex-col items-start gap-0.5 border p-2.5 text-left transition-colors",
								picked
									? "border-primary bg-primary/10"
									: "border-base-300 bg-base-100 hover:border-primary/50",
							]}
							onclick={() => (loginMethod = o.method)}
						>
							<span class="flex items-center gap-1.5 text-sm font-medium">
								<o.Icon
									size={14}
									class={picked ? "text-primary" : "text-base-content/50"}
								/>
								{o.title()}
							</span>
							<span class="text-base-content/60 text-xs leading-snug">{o.hint()}</span
							>
						</button>
					{/each}
				</div>
			</fieldset>
		{/if}

		<div class={["grid gap-3", mode === "download" && "grid-cols-2"]}>
			<label class="flex flex-col gap-1">
				<span class="text-base-content/60 flex justify-between text-xs">
					<span>{m.wizard_instance_name()}</span>
					<span>{m.common_optional()}</span>
				</span>
				<input
					type="text"
					class="input input-sm w-full"
					placeholder={patch ? freeLabel(patch.name) : m.wizard_name_placeholder()}
					bind:value={name}
				/>
			</label>
			{#if mode === "download"}
				<div class="flex flex-col gap-1">
					<label
						for="wizard-install-folder"
						class="text-base-content/60 flex justify-between text-xs"
					>
						<span>{m.wizard_installation_path()}</span>
						<span>{m.common_optional()}</span>
					</label>
					<div class="join w-full">
						<input
							id="wizard-install-folder"
							type="text"
							class="input input-sm join-item min-w-0 flex-1 font-mono"
							spellcheck="false"
							placeholder={defaultInstancePath.value || undefined}
							bind:value={installFolder}
						/>
						<button
							type="button"
							class="btn btn-sm btn-square join-item"
							aria-label={m.common_browse()}
							title={m.common_browse()}
							onclick={browseInstallFolder}
						>
							<FolderOpen size={14} />
						</button>
					</div>
				</div>
				{#if installTarget}
					<p class="text-base-content/60 col-span-2 -mt-1 text-xs wrap-break-word">
						{installTarget.copy
							? m.wizard_installs_copy_to({ path: installTarget.path })
							: m.wizard_installs_to({ path: installTarget.path })}
					</p>
				{/if}
			{/if}
		</div>

		{#if mode === "download" && downloadPatch}
			<details class="group bg-base-100 border-base-300 rounded-field border">
				<summary
					class="text-base-content/70 hover:text-base-content flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-sm [&::-webkit-details-marker]:hidden"
				>
					<Terminal size={14} />
					{m.wizard_depot_downloader_command()}
					<ChevronDown
						size={14}
						class="ml-auto opacity-60 transition-transform group-open:rotate-180"
					/>
				</summary>
				<div class="border-base-300 flex flex-col gap-2 border-t p-3">
					<p class="text-base-content/60 text-xs">{m.wizard_depot_downloader_hint()}</p>
					<div class="bg-base-200 rounded-field relative">
						<pre
							class="overflow-x-auto py-2 pr-20 pl-3 font-mono text-xs wrap-break-word whitespace-pre-wrap select-text">{depotDownloaderCommand}</pre>
						<button
							type="button"
							class="btn btn-ghost btn-xs absolute top-1.5 right-1.5"
							onclick={handleCopyCommand}
						>
							{#if copyStatus === "copied"}
								<Check size={12} />
								{m.common_copied()}
							{:else}
								<Copy size={12} />
								{copyStatus === "failed" ? m.common_copy_failed() : m.common_copy()}
							{/if}
						</button>
					</div>
				</div>
			</details>
		{/if}
	</div>

	{#snippet actions()}
		<p
			class="text-base-content/55 mr-auto flex min-w-0 items-center gap-1.5 self-center text-xs"
		>
			<Info size={12} class="shrink-0" />
			{mode === "download" ? m.wizard_download_footer() : m.wizard_import_footer()}
		</p>
		<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}>
			{m.common_cancel()}
		</button>
		<button class="btn btn-primary btn-sm" type="submit" disabled={!ready}>
			{#if mode === "download"}
				<CloudDownload size={14} />
				{m.wizard_download_and_create()}
			{:else if detecting}
				<span class="loading loading-spinner loading-xs"></span>
				{m.common_identifying()}
			{:else}
				<FolderOpen size={14} />
				{bulk
					? m.wizard_import_count({ count: chosen.length })
					: m.wizard_import_instance()}
			{/if}
		</button>
	{/snippet}
</Modal>
