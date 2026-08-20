<script lang="ts">
	import { Box, Folder } from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { resolveResource } from "@tauri-apps/api/path";
	import { open as openDialog } from "@tauri-apps/plugin-dialog";
	import { installMod, removeMod, type ModRecord } from "$lib/core/mods";
	import { m } from "$lib/paraglide/messages";
	import { createInstancePlatformsQuery } from "$lib/queries/instance";
	import { createModsQuery } from "$lib/queries/mods";
	import { updateInstance } from "$lib/stores/instance.svelte";
	import { parseArgs } from "$lib/utils/args";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import type { Instance, InstancePlatform } from "$lib/types/instance";

	interface Props {
		instance: Instance;
		// Re-sync the form from the instance whenever this becomes true (modal open / tab shown).
		active?: boolean;
		// Called after a successful save (the modal uses it to close itself).
		onSaved?: () => void;
		// Called when the user discards edits (the modal uses it to close itself).
		onCancel?: () => void;
	}

	let { instance, active = true, onSaved, onCancel }: Props = $props();

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

	const queryClient = useQueryClient();

	let editName = $state("");
	let editVersion = $state("");
	let editPath = $state("");
	let editPlatform = $state<InstancePlatform>("Win64");
	let editArgsText = $state("");
	let editColor = $state("");
	let editEnableConsole = $state(false);
	let initialEnableConsole = false;
	let editEnableCore = $state(false);
	let initialEnableCore = false;
	let hasInitializedMods = $state(false);

	const modsQuery = createModsQuery(() => editPath);

	function syncFromInstance() {
		editName = instance.label;
		editVersion = instance.version || "";
		editPath = instance.path;
		editPlatform = instance.launchOptions?.platform ?? "Win64";
		editArgsText = (instance.launchOptions?.args ?? []).join(" ");
		editColor = getInstanceColor(instance);
		hasInitializedMods = false;
	}

	// ponytail: reset the form whenever the panel becomes active with the latest instance data
	$effect(() => {
		if (!active) return;
		syncFromInstance();
	});

	let isCoreVersion = $derived(editVersion === "0.56" || editVersion === "0.57");

	// Which installed mods count as the bundled Console / Multiplayer mods,
	// matched by source path or legacy internal name.
	const isConsoleMod = (mod: ModRecord) =>
		mod.OriginalPath.includes("Tempest Console.tempest") ||
		mod.Name === "Tempest Mod (Console)" ||
		mod.OriginalPath.includes("Tempest Mod.tempest") ||
		mod.Name === "Tempest Mod (Console + Multiplayer)";
	const isCoreMod = (mod: ModRecord) => mod.OriginalPath.includes("Tempest Core.tempest");

	// ponytail: detect installed bundled mods using createModsQuery
	$effect(() => {
		if (active && modsQuery.data && !hasInitializedMods) {
			const consoleInstalled = modsQuery.data.some(isConsoleMod);
			editEnableConsole = consoleInstalled;
			initialEnableConsole = consoleInstalled;

			const coreInstalled = modsQuery.data.some(isCoreMod);
			editEnableCore = coreInstalled;
			initialEnableCore = coreInstalled;

			hasInitializedMods = true;
		}
	});

	// Remove every installed mod matching the predicate by its real internal name
	// (removing by a hard-coded name silently fails when the name doesn't match).
	async function removeMatchingMods(isMatch: (mod: ModRecord) => boolean): Promise<void> {
		for (const mod of modsQuery.data ?? []) {
			if (isMatch(mod)) {
				await removeMod(editPath, mod.Name);
			}
		}
	}

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

	let availablePlatforms = $derived(
		(editPath ? platformsQuery.data : undefined) ?? ([] as InstancePlatform[]),
	);
	let isDetectingPlatforms = $derived(platformsQuery.isFetching);

	$effect(() => {
		if (!availablePlatforms.length) return;
		if (!availablePlatforms.includes(editPlatform)) {
			editPlatform = availablePlatforms[0] ?? "Win64";
		}
	});

	async function save() {
		updateInstance(instance.id, {
			label: editName,
			version: editVersion,
			path: editPath,
			color: editColor,
			launchOptions: {
				...instance.launchOptions,
				platform: editPlatform,
				args: parseArgs(editArgsText),
			},
		});

		// ponytail: install or remove the bundled mod if the checkbox toggled.
		// 0.56/0.57 use a single "Console + Multiplayer" toggle backed by Tempest Core.
		if (isCoreVersion) {
			if (editEnableCore !== initialEnableCore) {
				try {
					if (editEnableCore) {
						const modFile = await resolveResource("Tempest Core.tempest");
						await installMod(editPath, modFile, true, true);
					} else {
						await removeMatchingMods(isCoreMod);
					}
					queryClient.invalidateQueries({ queryKey: ["mods", editPath] });
				} catch (error) {
					console.error("Failed to toggle Core mod:", error);
				}
			}
		} else if (editEnableConsole !== initialEnableConsole) {
			try {
				if (editEnableConsole) {
					const modFile = await resolveResource("Tempest Console.tempest");
					await installMod(editPath, modFile, true, true);
				} else {
					await removeMatchingMods(isConsoleMod);
				}
				queryClient.invalidateQueries({ queryKey: ["mods", editPath] });
			} catch (error) {
				console.error("Failed to toggle Console mod:", error);
			}
		}

		onSaved?.();
	}

	function handleCancel() {
		// Discard unsaved edits.
		syncFromInstance();
		onCancel?.();
	}
</script>

<div class="flex flex-col gap-4">
	<div class="form-control">
		<label for="instance-name" class="label py-0.5">
			<span class="label-text text-sm">{m.instance_name()}</span>
		</label>
		<input
			id="instance-name"
			type="text"
			placeholder={m.instance_name()}
			class="input input-bordered w-full"
			bind:value={editName}
		/>
	</div>

	<div class="form-control">
		<label for="instance-version" class="label py-0.5">
			<span class="label-text text-sm">{m.instance_version()}</span>
		</label>
		<input
			id="instance-version"
			type="text"
			placeholder="1.0.0"
			class="input input-bordered w-full"
			bind:value={editVersion}
		/>
	</div>

	<div class="form-control">
		<label for="instance-color" class="label py-0.5">
			<span class="label-text text-sm">{m.instance_color()}</span>
		</label>
		<div class="flex w-full items-center gap-6">
			<div
				class="border-base-content/10 flex h-20 w-20 shrink-0 items-center justify-center overflow-hidden rounded-xl border shadow-sm"
				style="background-color: {editColor};"
			>
				<Box size={40} style="color: {getContrastColor(editColor)};" />
			</div>

			<div class="flex h-20 min-w-0 flex-1 flex-col justify-between">
				<div class="flex w-full flex-wrap justify-between">
					{#each colorPresets as preset}
						<button
							type="button"
							class="border-base-content/10 flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-full border transition-transform hover:scale-110 active:scale-95"
							style="background-color: {preset};"
							onclick={() => (editColor = preset)}
							title={preset}
						>
							{#if editColor.toLowerCase() === preset.toLowerCase()}
								<span class="text-[10px] font-bold text-white">✓</span>
							{/if}
						</button>
					{/each}
				</div>

				<div class="flex w-full items-center gap-2">
					<input
						id="instance-color"
						type="color"
						class="border-base-300 bg-base-100 h-10 w-16 shrink-0 cursor-pointer rounded border p-0.5"
						bind:value={editColor}
					/>
					<button
						type="button"
						class="btn btn-sm flex-1"
						onclick={() => {
							editColor = getInstanceColor({
								...instance,
								label: editName,
								color: undefined,
							});
						}}
					>
						{m.instance_reset_color()}
					</button>
				</div>
			</div>
		</div>
	</div>

	<div class="form-control">
		<label for="instance-path" class="label py-0.5">
			<span class="label-text text-sm">{m.instance_installation_path()}</span>
		</label>
		<div class="join w-full">
			<input
				id="instance-path"
				type="text"
				placeholder="/path/to/instance"
				class="input input-bordered join-item flex-1 font-mono"
				bind:value={editPath}
			/>
			<button class="btn btn-accent join-item" type="button" onclick={handleBrowse}>
				<Folder size={16} />
				{m.common_browse()}
			</button>
		</div>
	</div>

	<div class="form-control">
		<label for="instance-args" class="label py-0.5">
			<span class="label-text text-sm">{m.instance_launch_arguments()}</span>
		</label>
		<div class="space-y-2">
			<input
				id="instance-args"
				type="text"
				placeholder=""
				class="input input-bordered w-full font-mono text-sm"
				bind:value={editArgsText}
			/>
			<p class="text-xs opacity-60">{m.instance_space_separated()}</p>
		</div>
	</div>

	{#if availablePlatforms.length > 1}
		<div class="form-control">
			<label for="instance-platform" class="label py-0.5">
				<span class="label-text text-sm">{m.instance_platform()}</span>
			</label>
			<select
				id="instance-platform"
				class="select select-bordered w-full"
				disabled={isDetectingPlatforms}
				bind:value={editPlatform}
			>
				{#each availablePlatforms as platform}
					<option value={platform}>{platform}</option>
				{/each}
			</select>
		</div>
	{/if}

	{#if isCoreVersion}
		<div class="form-control">
			<label class="label cursor-pointer justify-start gap-3 py-0.5">
				<input type="checkbox" class="toggle toggle-accent" bind:checked={editEnableCore} />
				<span class="label-text text-sm font-semibold">Enable Console + Multiplayer</span>
			</label>
		</div>
	{:else}
		<div class="form-control">
			<label class="label cursor-pointer justify-start gap-3 py-0.5">
				<input
					type="checkbox"
					class="toggle toggle-accent"
					bind:checked={editEnableConsole}
				/>
				<span class="label-text text-sm font-semibold">Enable Console</span>
			</label>
		</div>
	{/if}
</div>

<div class="flex w-full justify-end gap-2 pt-4">
	<button class="btn btn-ghost" type="button" onclick={handleCancel}>
		{m.common_cancel()}
	</button>
	<button class="btn btn-accent" type="button" onclick={save}>
		{m.common_save_changes()}
	</button>
</div>
