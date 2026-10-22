<script lang="ts">
	import { goto } from "$app/navigation";
	import {
		BookOpen,
		Check,
		Cpu,
		ExternalLink,
		FolderOpen,
		Palette,
		RefreshCw,
		Rocket,
		RotateCcw,
		Save,
		ShieldCheck,
		SlidersHorizontal,
		Terminal,
		Trash2,
		Wrench,
	} from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { onMount, untrack } from "svelte";
	import DeleteInstanceDialog from "$lib/components/library/DeleteInstanceDialog.svelte";
	import ActionRow from "$lib/components/ui/ActionRow.svelte";
	import PaladinsIcon from "$lib/components/ui/PaladinsIcon.svelte";
	import Panel from "$lib/components/ui/Panel.svelte";
	import SectionNav from "$lib/components/ui/SectionNav.svelte";
	import { SectionTracker, type PageSection } from "$lib/components/ui/sections.svelte";
	import SwitchRow from "$lib/components/ui/SwitchRow.svelte";
	import { deleteInstance } from "$lib/core/instance-delete";
	import { manifestIdOf, runSetup, verifyFiles } from "$lib/core/instance-maintenance";
	import { installMod, removeMod } from "$lib/core/mods";
	import versions from "$lib/data/versions.json";
	import { relativeDate } from "$lib/mods/listing";
	import { m } from "$lib/paraglide/messages";
	import { getLocale } from "$lib/paraglide/runtime";
	import { open as openDialog, openUrl, resolveResource } from "$lib/platform/api";
	import { createInstancePlatformsQuery } from "$lib/queries/instance";
	import { createModsQuery } from "$lib/queries/mods";
	import { WIKI_BASE_URL } from "$lib/rigby/constants";
	import { updateInstance } from "$lib/stores/instance.svelte";
	import { addToast } from "$lib/stores/ui.svelte";
	import { instancePlatforms, type Instance, type InstancePlatform } from "$lib/types/instance";
	import { parseArgs } from "$lib/utils/args";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import { patchOf } from "$lib/utils/versions";
	import type { ModInstallResult, ModRecord } from "$lib/core/mods";

	interface Props {
		/** The page keys this component on the instance id, so the fields load from it once */
		instance: Instance;
		isRunning: boolean;
	}

	let { instance, isRunning }: Props = $props();

	const colorPresets = [
		"#ef4444",
		"#f97316",
		"#f59e0b",
		"#10b981",
		"#06b6d4",
		"#3b82f6",
		"#6366f1",
		"#8b5cf6",
		"#ec4899",
		"#6b7280",
	];

	const platformHints: Record<InstancePlatform, () => string> = {
		Win64: m.instance_platform_win64,
		Win32: m.instance_platform_win32,
	};

	const sections: PageSection[] = $derived([
		{ id: "general", title: m.settings_general(), icon: SlidersHorizontal },
		{ id: "appearance", title: m.settings_appearance(), icon: Palette },
		{ id: "launching", title: m.settings_launching(), icon: Rocket },
		{ id: "maintenance", title: m.instance_maintenance(), icon: Wrench },
		{ id: "remove", title: m.instance_remove_title(), icon: Trash2, tone: "danger" },
	]);
	const tracker = new SectionTracker(() => sections);

	const queryClient = useQueryClient();

	let editName = $state("");
	let editVersion = $state("");
	let editPath = $state("");
	let editPlatform = $state<InstancePlatform>("Win64");
	let editArgsText = $state("");
	let editColor = $state("");

	const fieldsOf = (i: Instance) =>
		JSON.stringify({
			label: i.label,
			version: i.version ?? "",
			path: i.path,
			color: getInstanceColor(i),
			platform: i.launchOptions?.platform ?? "Win64",
			args: i.launchOptions?.args ?? [],
		});
	const stored = $derived(fieldsOf(instance));
	// What the form last loaded or saved; a stored value that is neither was changed elsewhere.
	let known = "";

	function syncFromInstance() {
		editName = instance.label;
		editVersion = instance.version || "";
		editPath = instance.path;
		editPlatform = instance.launchOptions?.platform ?? "Win64";
		editArgsText = (instance.launchOptions?.args ?? []).join(" ");
		editColor = getInstanceColor(instance);
		known = stored;
	}
	syncFromInstance();

	// E.g. setup adding the shooting range argument: take it in rather than save over it.
	$effect(() => {
		if (stored !== known) untrack(syncFromInstance);
	});

	function buildUpdates() {
		return {
			label: editName.trim() || instance.label,
			version: editVersion.trim(),
			path: editPath,
			color: editColor,
			launchOptions: {
				...instance.launchOptions,
				platform: editPlatform,
				args: parseArgs(editArgsText),
			},
		};
	}

	// Auto-save: edits are written shortly after they stop changing, and on leaving the tab.
	let pending: { id: string; updates: ReturnType<typeof buildUpdates>; fields: string } | null =
		null;
	let pendingTimer: ReturnType<typeof setTimeout> | undefined;

	function flush() {
		clearTimeout(pendingTimer);
		if (!pending) return;
		known = pending.fields;
		updateInstance(pending.id, pending.updates);
		pending = null;
	}

	onMount(() => flush);

	$effect(() => {
		const updates = buildUpdates();
		const fields = fieldsOf({ ...instance, ...updates });
		clearTimeout(pendingTimer);
		// Edited and back again before the write: nothing to save, not the edit in between.
		if (fields === stored) {
			pending = null;
			return;
		}
		pending = { id: instance.id, updates, fields };
		pendingTimer = setTimeout(flush, 400);
	});

	const patch = $derived(patchOf(editVersion.trim()));
	const versionHint = $derived.by(() => {
		if (!patch) return m.instance_version_unknown();
		const released = new Date(patch.date).toLocaleDateString(getLocale(), {
			month: "long",
			year: "numeric",
		});
		return `${m.library_patch_named({ version: patch.version, name: patch.name })} · ${released}`;
	});

	const contrast = $derived(getContrastColor(editColor));
	const defaultColor = $derived(
		getInstanceColor({
			...instance,
			label: editName.trim() || instance.label,
			color: undefined,
		}),
	);
	const customColor = $derived(!colorPresets.includes(editColor.toLowerCase()));

	async function handleBrowse() {
		const result = await openDialog({
			directory: true,
			multiple: false,
			title: m.settings_select_instance_folder(),
		});
		if (result) {
			editPath = result;
		}
	}

	const platformsQuery = createInstancePlatformsQuery(() => editPath);
	const availablePlatforms = $derived(
		(editPath ? platformsQuery.data : undefined) ?? ([] as InstancePlatform[]),
	);

	$effect(() => {
		if (!availablePlatforms.length) return;
		if (!availablePlatforms.includes(editPlatform)) {
			editPlatform = availablePlatforms[0] ?? "Win64";
		}
	});

	const modsQuery = createModsQuery(() => editPath);
	const mods = $derived(modsQuery.data ?? []);
	const enabledMods = $derived(mods.filter((mod) => mod.Enabled).length);

	// The bundled Console / Multiplayer mods, matched by source path or legacy internal name.
	const isConsoleMod = (mod: ModRecord) =>
		mod.OriginalPath.includes("Tempest Console.tempest") ||
		mod.Name === "Tempest Mod (Console)" ||
		mod.OriginalPath.includes("Tempest Mod.tempest") ||
		mod.Name === "Tempest Mod (Console + Multiplayer)";
	const isCoreMod = (mod: ModRecord) =>
		mod.OriginalPath.includes("Tempest Core.tempest") ||
		mod.OriginalPath.includes("Tempest.Core.Tempest");

	const isCoreVersion = $derived(editVersion === "0.56" || editVersion === "0.57");
	const bundledInstalled = $derived(mods.some(isCoreVersion ? isCoreMod : isConsoleMod));
	const bundledTitle = $derived(
		isCoreVersion ? m.instance_bundled_core() : m.instance_bundled_console(),
	);
	// Where the switch stands while the bundled mod is being installed or removed.
	let bundledPending = $state<boolean | null>(null);

	async function setBundledMod(enable: boolean) {
		if (bundledPending !== null) return;
		bundledPending = enable;
		const isMatch = isCoreVersion ? isCoreMod : isConsoleMod;
		// The CLI reports a failure in its result rather than by exiting.
		const check = (result: ModInstallResult) => {
			if (!result.Success) throw new Error(result.Message);
		};
		try {
			if (enable) {
				const file = isCoreVersion
					? "mods/Tempest Core.tempest"
					: "mods/Tempest Console.tempest";
				check(await installMod(editPath, await resolveResource(file), true));
			} else {
				// By each mod's real internal name: removing by a hard-coded one silently fails.
				for (const mod of mods.filter(isMatch)) {
					check(await removeMod(editPath, mod.Name));
				}
			}
		} catch (error) {
			console.error("Failed to toggle bundled mod:", error);
			addToast({
				title: bundledTitle,
				message: error instanceof Error ? error.message : String(error),
				tone: "error",
			});
		} finally {
			// Also after a failure: a removal can stop halfway.
			await queryClient.invalidateQueries({ queryKey: ["mods", editPath] });
			bundledPending = null;
		}
	}

	const phase = $derived(instance.state.type);
	// Both write into the game folder, so neither while it is in use.
	const maintenanceBlocked = $derived(isRunning || phase !== "prepared");
	const canVerify = $derived(!!(instance.path && manifestIdOf(instance)));

	function handleSetup() {
		flush();
		void runSetup(instance);
	}

	function handleVerify() {
		flush();
		if (verifyFiles(instance)) void goto("/downloads");
	}

	let removeOpen = $state(false);

	const storedPatch = $derived(patchOf(instance.version));
	const wikiReference = $derived(storedPatch?.wikiReference);
	const released = $derived(
		storedPatch
			? new Date(storedPatch.date).toLocaleDateString(getLocale(), {
					month: "short",
					year: "numeric",
				})
			: "–",
	);
	const lastPlayed = $derived.by(() => {
		if (isRunning) return m.library_badge_running();
		if (!instance.lastPlayedAt) return m.instance_never_played();
		return relativeDate(new Date(instance.lastPlayedAt).toISOString());
	});
</script>

<div
	class="@container min-h-0 flex-1 overflow-y-auto"
	onscroll={tracker.track}
	onscrollend={tracker.settle}
	onwheel={tracker.settle}
>
	<div
		class="mx-auto grid w-full max-w-6xl items-start gap-4 px-4 py-6 @3xl:grid-cols-[minmax(0,1fr)_18rem]"
	>
		<div class="flex min-w-0 flex-col gap-4">
			<Panel {...tracker.panel("general")}>
				<div class="grid items-start gap-3 @lg:grid-cols-2">
					<label class="flex flex-col gap-1">
						<span class="text-base-content/60 text-xs">{m.instance_name()}</span>
						<input
							type="text"
							class="input input-sm w-full"
							bind:value={editName}
							placeholder={instance.label}
						/>
					</label>
					<div class="flex flex-col gap-1">
						<label for="instance-version" class="text-base-content/60 text-xs"
							>{m.instance_version()}</label
						>
						<input
							id="instance-version"
							type="text"
							list="instance-patches"
							class="input input-sm w-full font-mono"
							spellcheck="false"
							bind:value={editVersion}
							placeholder="8.1"
						/>
						<datalist id="instance-patches">
							{#each versions as v (v.id)}
								<option value={v.version}>{v.name}</option>
							{/each}
						</datalist>
						<p class={["text-xs", patch ? "text-base-content/60" : "text-warning"]}>
							{versionHint}
						</p>
					</div>
					<div class="flex flex-col gap-1 @lg:col-span-2">
						<label for="instance-path" class="text-base-content/60 text-xs"
							>{m.instance_installation_path()}</label
						>
						<div class="join w-full">
							<input
								id="instance-path"
								type="text"
								class="input input-sm join-item min-w-0 flex-1 font-mono"
								spellcheck="false"
								bind:value={editPath}
							/>
							<button
								type="button"
								class="btn btn-sm join-item"
								onclick={handleBrowse}
							>
								<FolderOpen size={14} />
								{m.common_browse()}
							</button>
						</div>
						<p class="text-base-content/60 text-xs">{m.instance_folder_hint()}</p>
					</div>
				</div>
			</Panel>

			<Panel {...tracker.panel("appearance")}>
				{#snippet aside()}
					<button
						type="button"
						class="btn btn-ghost btn-xs"
						disabled={editColor === defaultColor}
						onclick={() => (editColor = defaultColor)}
					>
						<RotateCcw size={12} />
						{m.instance_reset_color()}
					</button>
				{/snippet}
				<div class="flex flex-wrap items-center gap-x-4 gap-y-3">
					<!-- The instance as the library lists it -->
					<div
						class="bg-base-100 border-base-300 rounded-field flex min-w-0 flex-1 basis-56 items-center gap-3 border p-3"
					>
						<span
							class="rounded-field flex size-10 shrink-0 items-center justify-center text-[11px] font-bold tabular-nums transition-colors"
							style="background-color: {editColor}; color: {contrast};"
						>
							{#if editVersion.trim()}
								{editVersion.trim()}
							{:else}
								<PaladinsIcon size={28} color={contrast} />
							{/if}
						</span>
						<span class="min-w-0">
							<span class="block truncate text-sm font-semibold"
								>{editName.trim() || instance.label}</span
							>
							<span class="text-base-content/60 block text-xs"
								>{m.instance_color_hint()}</span
							>
						</span>
					</div>
					<div class="flex flex-wrap items-center gap-2">
						<div
							class="flex flex-wrap items-center gap-2"
							role="radiogroup"
							aria-label={m.instance_color()}
						>
							{#each colorPresets as preset (preset)}
								{@const active = editColor.toLowerCase() === preset}
								<button
									type="button"
									role="radio"
									aria-checked={active}
									aria-label={preset}
									title={preset}
									class={[
										"flex size-7 cursor-pointer items-center justify-center rounded-full transition-transform hover:scale-110 active:scale-95",
										active &&
											"ring-base-content/50 ring-offset-base-200 ring-2 ring-offset-2",
									]}
									style="background-color: {preset}; color: {getContrastColor(
										preset,
									)};"
									onclick={() => (editColor = preset)}
								>
									{#if active}<Check size={14} />{/if}
								</button>
							{/each}
						</div>
						<label
							class={[
								"relative flex size-7 cursor-pointer items-center justify-center rounded-full transition-transform hover:scale-110",
								customColor &&
									"ring-base-content/50 ring-offset-base-200 ring-2 ring-offset-2",
							]}
							style={customColor
								? `background-color: ${editColor}; color: ${contrast};`
								: "background: conic-gradient(#ef4444, #f59e0b, #10b981, #06b6d4, #6366f1, #ec4899, #ef4444);"}
							title={m.instance_color_custom()}
						>
							{#if customColor}<Check size={14} />{/if}
							<input
								type="color"
								class="absolute inset-0 size-full cursor-pointer opacity-0"
								aria-label={m.instance_color_custom()}
								bind:value={editColor}
							/>
						</label>
					</div>
				</div>
			</Panel>

			<Panel {...tracker.panel("launching")}>
				<div class="flex flex-col gap-3">
					<div class="flex flex-col gap-1">
						<span id="instance-platform-label" class="text-base-content/60 text-xs"
							>{m.instance_platform()}</span
						>
						<div
							class="grid gap-2 @md:grid-cols-2"
							role="radiogroup"
							aria-labelledby="instance-platform-label"
						>
							{#each instancePlatforms as platform (platform)}
								{@const available =
									availablePlatforms.length === 0 ||
									availablePlatforms.includes(platform)}
								{@const active = editPlatform === platform}
								<button
									type="button"
									role="radio"
									aria-checked={active}
									disabled={!available}
									class={[
										"bg-base-100 rounded-field flex items-center gap-3 border p-3 text-left transition-colors",
										active
											? "border-primary ring-primary/20 ring-2"
											: "border-base-300",
										!available && "opacity-50",
										available &&
											!active &&
											"hover:border-primary/50 cursor-pointer",
									]}
									onclick={() => (editPlatform = platform)}
								>
									<span
										class={[
											"flex size-8 shrink-0 items-center justify-center rounded-full transition-colors",
											active
												? "bg-primary/15 text-primary"
												: "bg-base-300 text-base-content/60",
										]}><Cpu size={16} /></span
									>
									<span class="min-w-0 flex-1">
										<span class="block text-sm font-medium">{platform}</span>
										<span class="text-base-content/60 block text-xs">
											{available
												? platformHints[platform]()
												: m.instance_platform_missing()}
										</span>
									</span>
									{#if active}<Check
											size={14}
											class="text-primary shrink-0"
										/>{/if}
								</button>
							{/each}
						</div>
					</div>
					<div class="flex flex-col gap-1">
						<label for="instance-args" class="text-base-content/60 text-xs"
							>{m.instance_launch_arguments()}</label
						>
						<input
							id="instance-args"
							type="text"
							class="input input-sm w-full font-mono"
							spellcheck="false"
							bind:value={editArgsText}
						/>
						<p class="text-base-content/60 text-xs">{m.instance_space_separated()}</p>
					</div>
					<SwitchRow
						bind:checked={
							() => bundledPending ?? bundledInstalled, (on) => void setBundledMod(on)
						}
						icon={Terminal}
						title={bundledTitle}
						hint={isCoreVersion
							? m.instance_bundled_core_hint()
							: m.instance_bundled_console_hint()}
						disabled={bundledPending !== null || isRunning || !editPath}
					/>
				</div>
			</Panel>

			<Panel {...tracker.panel("maintenance")}>
				<div class="flex flex-col gap-2">
					<ActionRow
						icon={RefreshCw}
						title={m.instance_setup_title()}
						hint={m.instance_setup_hint()}
					>
						<button
							type="button"
							class="btn btn-sm shrink-0"
							disabled={maintenanceBlocked}
							onclick={handleSetup}
						>
							{#if phase === "setup"}
								<span class="loading loading-spinner loading-xs"></span>
							{/if}
							{m.instancemenu_run_setup()}
						</button>
					</ActionRow>
					<ActionRow
						icon={ShieldCheck}
						title={m.instance_verify_title()}
						hint={canVerify ? m.instance_verify_hint() : m.instance_verify_unknown()}
					>
						<button
							type="button"
							class="btn btn-sm shrink-0"
							disabled={maintenanceBlocked || !canVerify}
							onclick={handleVerify}
						>
							{m.instancemenu_verify()}
						</button>
					</ActionRow>
				</div>
			</Panel>

			<Panel {...tracker.panel("remove")}>
				<p class="text-base-content/70 mb-3 text-sm">{m.instance_remove_hint()}</p>
				<button
					type="button"
					class="btn btn-error btn-soft btn-sm"
					onclick={() => (removeOpen = true)}
				>
					<Trash2 size={14} />
					{m.common_remove()}…
				</button>
			</Panel>
		</div>

		<aside class="flex flex-col gap-3 @3xl:sticky @3xl:top-6">
			<div class="bg-base-200 border-base-300 rounded-box flex flex-col gap-3 border p-4">
				<div class="@max-3xl:hidden">
					<SectionNav {tracker} />
				</div>
				{#if wikiReference}
					<button
						type="button"
						class="btn btn-sm w-full"
						onclick={() =>
							openUrl(
								`${WIKI_BASE_URL}${encodeURIComponent(wikiReference)}?fandom=allow`,
							)}
					>
						<BookOpen size={14} />
						{m.instancemenu_open_wiki()}
						<ExternalLink size={12} class="opacity-60" />
					</button>
				{/if}
				<p class="text-base-content/60 flex items-center gap-1 text-xs">
					<Save size={12} class="shrink-0" />
					{m.instance_autosave()}
				</p>
			</div>
			<dl
				class="bg-base-200 border-base-300 rounded-box grid grid-cols-2 gap-3 border p-4 text-sm"
			>
				<div class="min-w-0">
					<dt class="text-base-content/60 text-xs">{m.instance_fact_patch()}</dt>
					<dd class="truncate font-mono font-semibold" title={storedPatch?.name}>
						{instance.version || "–"}
					</dd>
				</div>
				<div class="min-w-0">
					<dt class="text-base-content/60 text-xs">{m.instance_fact_released()}</dt>
					<dd class="truncate font-semibold">{released}</dd>
				</div>
				<div class="min-w-0">
					<dt class="text-base-content/60 text-xs">{m.instance_content()}</dt>
					<dd
						class="truncate font-semibold tabular-nums"
						title={m.library_mods_enabled({ enabled: enabledMods, count: mods.length })}
					>
						{mods.length}
					</dd>
				</div>
				<div class="min-w-0">
					<dt class="text-base-content/60 text-xs">{m.instance_fact_played()}</dt>
					<dd
						class="truncate font-semibold"
						class:text-success={isRunning}
						title={instance.lastPlayedAt
							? new Date(instance.lastPlayedAt).toLocaleString()
							: undefined}
					>
						{lastPlayed}
					</dd>
				</div>
			</dl>
		</aside>
	</div>
</div>

<DeleteInstanceDialog
	bind:open={removeOpen}
	instanceName={instance.label}
	onconfirm={(mode) => deleteInstance(instance, mode)}
/>
