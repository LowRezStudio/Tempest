<script lang="ts">
	import { Box, Folder } from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { resolveResource } from "@tauri-apps/api/path";
	import { open as openDialog } from "@tauri-apps/plugin-dialog";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { installMod, removeMod } from "$lib/core/mods";
	import { m } from "$lib/paraglide/messages";
	import { createInstancePlatformsQuery } from "$lib/queries/instance";
	import { createModsQuery } from "$lib/queries/mods";
	import { updateInstance } from "$lib/stores/instance.svelte";
	import { parseArgs } from "$lib/utils/args";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import type { Instance, InstancePlatform } from "$lib/types/instance";

	interface Props {
		instance: Instance;
		open?: boolean;
	}

	let { instance, open = $bindable(false) }: Props = $props();

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
	let editArgs = $state<string[]>([]);
	let editColor = $state("");
	let argsInput = $state("");
	let editEnableConsole = $state(false);
	let initialEnableConsole = false;
	let editEnableMultiplayer = $state(false);
	let initialEnableMultiplayer = false;
	let hasInitializedMods = false;

	const modsQuery = createModsQuery(() => editPath);

	// ponytail: reset form whenever the modal opens with the latest instance data
	$effect(() => {
		if (open) {
			editName = instance.label;
			editVersion = instance.version || "";
			editPath = instance.path;
			editPlatform = instance.launchOptions?.platform ?? "Win64";
			editArgs = instance.launchOptions?.args ?? [];
			editColor = getInstanceColor(instance);
			argsInput = "";
			hasInitializedMods = false;
		}
	});

	let showMultiplayer = $derived(editVersion === "0.56" || editVersion === "0.57");

	// ponytail: detect installed console/multiplayer mods using createModsQuery
	$effect(() => {
		if (open && modsQuery.data && !hasInitializedMods) {
			const consoleInstalled = modsQuery.data.some(
				(m) =>
					m.OriginalPath.includes("Console Mod.tempest") ||
					m.Name === "Tempest Mod (Console)" ||
					m.OriginalPath.includes("Tempest Mod.tempest") ||
					m.Name === "Tempest Mod (Console + Multiplayer)",
			);
			editEnableConsole = consoleInstalled;
			initialEnableConsole = consoleInstalled;

			const mpInstalled = modsQuery.data.some(
				(m) =>
					m.OriginalPath.includes("Multiplayer Mod.tempest") ||
					m.Name === "Tempest Mod (Multiplayer)",
			);
			editEnableMultiplayer = mpInstalled;
			initialEnableMultiplayer = mpInstalled;

			hasInitializedMods = true;
		}
	});

	function addArgs() {
		if (!argsInput.trim()) return;
		const newArgs = parseArgs(argsInput);
		editArgs = [...editArgs, ...newArgs];
		argsInput = "";
	}

	function removeArg(index: number) {
		editArgs = editArgs.filter((_, i) => i !== index);
	}

	function moveArg(index: number, direction: -1 | 1) {
		const newIndex = index + direction;
		if (newIndex < 0 || newIndex >= editArgs.length) return;
		const newArgs = [...editArgs];
		[newArgs[index], newArgs[newIndex]] = [newArgs[newIndex], newArgs[index]];
		editArgs = newArgs;
	}

	function handleArgsKeydown(e: KeyboardEvent) {
		if (e.key === "Enter") {
			e.preventDefault();
			addArgs();
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
				args: editArgs,
			},
		});

		// ponytail: install or remove Console mod if checkbox toggled
		if (editEnableConsole !== initialEnableConsole) {
			try {
				if (editEnableConsole) {
					const modFile = await resolveResource("Console Mod.tempest");
					await installMod(editPath, modFile, true, true);
				} else {
					await removeMod(editPath, "Tempest Mod (Console)");
					try {
						await removeMod(editPath, "Tempest Mod (Console + Multiplayer)");
					} catch {}
				}
				queryClient.invalidateQueries({ queryKey: ["mods", editPath] });
			} catch (error) {
				console.error("Failed to toggle Console mod:", error);
			}
		}

		// ponytail: install or remove Multiplayer mod if checkbox toggled
		if (showMultiplayer && editEnableMultiplayer !== initialEnableMultiplayer) {
			try {
				if (editEnableMultiplayer) {
					const modFile = await resolveResource("Multiplayer Mod.tempest");
					await installMod(editPath, modFile, true, true);
				} else {
					await removeMod(editPath, "Tempest Mod (Multiplayer)");
				}
				queryClient.invalidateQueries({ queryKey: ["mods", editPath] });
			} catch (error) {
				console.error("Failed to toggle Multiplayer mod:", error);
			}
		}

		open = false;
	}
</script>

<Modal bind:open title={m.instance_instance_settings()} class="max-w-2xl" onsubmit={save}>
	<div class="space-y-4">
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
				<div class="join w-full">
					<input
						id="instance-args"
						type="text"
						placeholder=""
						class="input input-bordered join-item flex-1 font-mono text-sm"
						bind:value={argsInput}
						onkeydown={handleArgsKeydown}
					/>
					<button class="btn btn-accent join-item" onclick={addArgs} type="button">
						{m.common_add()}
					</button>
				</div>
				{#if editArgs.length > 0}
					<div class="flex flex-wrap gap-1.5">
						{#each editArgs as arg, i (i)}
							<span class="badge badge-ghost badge-neutral gap-1">
								<button
									type="button"
									class="btn btn-ghost btn-xs btn-square text-base-content/60 hover:text-base-content h-4 min-h-0 w-4 p-0"
									onclick={() => moveArg(i, -1)}
									disabled={i === 0}
								>
									&#8592;
								</button>
								<span class="font-mono text-xs">{arg}</span>
								<button
									type="button"
									class="btn btn-ghost btn-xs btn-square text-base-content/60 hover:text-base-content h-4 min-h-0 w-4 p-0"
									onclick={() => moveArg(i, 1)}
									disabled={i === editArgs.length - 1}
								>
									&#8594;
								</button>
								<button
									type="button"
									class="btn btn-ghost btn-xs btn-square text-base-content/60 hover:text-base-content h-4 min-h-0 w-4 p-0"
									onclick={() => removeArg(i)}
								>
									&times;
								</button>
							</span>
						{/each}
					</div>
				{/if}
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

		<div class="form-control">
			<label class="label cursor-pointer justify-start gap-3 py-0.5">
				<input
					type="checkbox"
					class="checkbox checkbox-accent checkbox-sm"
					bind:checked={editEnableConsole}
				/>
				<span class="label-text text-sm font-semibold">Enable Console</span>
			</label>
		</div>

		{#if showMultiplayer}
			<div class="form-control">
				<label class="label cursor-pointer justify-start gap-3 py-0.5">
					<input
						type="checkbox"
						class="checkbox checkbox-accent checkbox-sm"
						bind:checked={editEnableMultiplayer}
					/>
					<span class="label-text text-sm font-semibold">Enable Multiplayer</span>
				</label>
			</div>
		{/if}
	</div>

	{#snippet actions()}
		<div class="flex w-full justify-end gap-2">
			<button class="btn btn-ghost" type="button" onclick={() => (open = false)}>
				{m.common_cancel()}
			</button>
			<button class="btn btn-accent" type="submit">
				{m.common_save_changes()}
			</button>
		</div>
	{/snippet}
</Modal>
