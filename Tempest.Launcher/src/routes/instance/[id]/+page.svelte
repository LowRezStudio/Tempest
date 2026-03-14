<script lang="ts">
	import {
		Box,
		EllipsisVertical,
		Folder,
		Gamepad2,
		PackageOpen,
		Play,
		RefreshCw,
		Settings,
		Square,
		Trash2,
	} from "@lucide/svelte";
	import { open as openDialog } from "@tauri-apps/plugin-dialog";
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import InstanceMenu from "$lib/components/library/InstanceMenu.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";
	import { createInstancePlatformsQuery } from "$lib/queries/instance";
	import { instanceMap, updateInstance } from "$lib/stores/instance";
	import { processesList } from "$lib/stores/processes";
	import { parseArgs } from "$lib/utils/args";
	import type { InstancePlatform } from "$lib/types/instance";

	type ModItem = {
		id: string;
		name: string;
		author: string;
		version: string;
		enabled: boolean;
		icon?: string;
	};

	let activeTab = $state<"content">("content");

	const mods = $state<ModItem[]>([
		{
			id: "1",
			name: "Multiplayer",
			author: "Tempest",
			version: "1.0.0",
			enabled: true,
		},
	]);

	const instance = $derived($instanceMap[page.params.id!]);
	let isSettingUp = $derived(
		(instance?.state as { type?: string } | undefined)?.type === "setup",
	);

	$effect(() => {
		if (!instance) {
			goto("/library");
		}
	});

	let isSettingsModalOpen = $state(false);
	let editName = $state("");
	let editVersion = $state("");
	let editPath = $state("");
	let editPlatform = $state<InstancePlatform>("Win64");
	let editArgs = $state<string[]>([]);
	let argsInput = $state("");

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

	function openSettings() {
		if (!instance) return;
		editName = instance.label;
		editVersion = instance.version || "";
		editPath = instance.path;
		editPlatform = instance.launchOptions?.platform ?? "Win64";
		editArgs = instance.launchOptions?.args ?? [];
		argsInput = "";
		isSettingsModalOpen = true;
	}

	function saveSettings() {
		if (!instance) return;
		updateInstance(instance.id, {
			label: editName,
			version: editVersion,
			path: editPath,
			launchOptions: {
				...instance.launchOptions,
				platform: editPlatform,
				args: editArgs,
			},
		});
		isSettingsModalOpen = false;
	}

	async function handleBrowse() {
		const result = await openDialog({
			directory: true,
			multiple: false,
			title: "Select Instance Folder",
		});
		if (result) {
			editPath = result;
		}
	}

	// Check if this instance is currently running
	let isRunning = $derived($processesList.some((p) => p.instance?.id === instance?.id));
	const launchGameMutation = createLaunchGameMutation();
	const killGameMutation = createKillGameMutation();
	let isLaunching = $derived(launchGameMutation.isPending);
	let isKilling = $derived(killGameMutation.isPending);
	let launchError = $derived(launchGameMutation.error?.message ?? "");
	let killError = $derived(killGameMutation.error?.message ?? "");

	function handleLaunchToggle() {
		if (!instance) return;
		if (isRunning) {
			killGameMutation.mutate(instance);
			return;
		}
		launchGameMutation.mutate(instance);
	}

	function clearLaunchError() {
		launchGameMutation.reset();
	}

	function clearKillError() {
		killGameMutation.reset();
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
</script>

<div class="flex flex-col h-full bg-base-100">
	<!-- Header -->
	<Header
		title={instance?.label || "Loading..."}
		tabs={[{ name: "Content", value: "content" }]}
		{activeTab}
		onSelectTab={(tab) => (activeTab = tab)}
	>
		{#snippet icon()}
			{#if isSettingUp}
				<span class="loading loading-spinner loading-md"></span>
			{:else}
				<Box size={32} class="opacity-60" />
			{/if}
		{/snippet}
		{#snippet actions()}
			<button
				class="btn text-sm"
				class:btn-accent={!isRunning}
				class:btn-error={isRunning}
				disabled={isLaunching || isKilling || isSettingUp}
				aria-busy={isLaunching || isKilling || isSettingUp}
				onclick={handleLaunchToggle}
			>
				{#if isLaunching}
					<span class="loading loading-spinner loading-xs"></span>
					Launching
				{:else if isKilling}
					<span class="loading loading-spinner loading-xs"></span>
					Stopping
				{:else if isRunning}
					<Square size={16} />
					Stop
				{:else}
					<Play size={16} />
					Play
				{/if}
			</button>
			<button class="btn btn-square" onclick={openSettings}>
				<Settings size={16} />
			</button>
			{#if instance}
				<InstanceMenu {instance} />
			{/if}
		{/snippet}
		{#snippet subtitle()}
			<div class="flex items-center gap-1.5">
				<Gamepad2 size={14} />
				<span>{instance?.version || "Unknown version"}</span>
			</div>
		{/snippet}
		{#snippet errors()}
			{#if launchError}
				<div class="pt-3">
					<div class="alert alert-error">
						<span>{launchError}</span>
						<button class="btn btn-ghost btn-sm" onclick={clearLaunchError}>
							Dismiss
						</button>
					</div>
				</div>
			{/if}
			{#if killError}
				<div class="pt-3">
					<div class="alert alert-error">
						<span>{killError}</span>
						<button class="btn btn-ghost btn-sm" onclick={clearKillError}>
							Dismiss
						</button>
					</div>
				</div>
			{/if}
		{/snippet}
	</Header>

	<!-- Content Area -->
	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		{#if activeTab === "content"}
			<div class="flex-1 overflow-y-auto">
				<div class="px-4 py-6">
					{#if mods.length === 0}
						<div class="flex flex-col items-center justify-center h-64 gap-4">
							<PackageOpen size={48} class="opacity-30" />
							<p class="text-lg text-base-content/50">No content installed</p>
							<p class="text-sm text-base-content/40">
								Add mods to customize your instance
							</p>
						</div>
					{:else}
						<table class="table">
							<thead>
								<tr>
									<th>
										<button
											class="flex items-center gap-1 font-semibold text-sm"
										>
											<span>Name</span>
										</button>
									</th>
									<th class="w-48">Version</th>
									<th class="w-auto text-right">
										<button class="btn btn-ghost btn-sm">
											<RefreshCw size={14} />
											Refresh
										</button>
									</th>
								</tr>
							</thead>
							<tbody>
								{#each mods as mod (mod.id)}
									<tr class="hover">
										<td>
											<div class="flex items-center gap-3">
												<div
													class="w-10 h-10 rounded-lg bg-base-200 flex items-center justify-center shrink-0"
												>
													<Box size={20} class="opacity-60" />
												</div>
												<div class="flex-1 min-w-0">
													<h3 class="font-bold text-sm truncate">
														{mod.name}
													</h3>
													<p class="text-xs opacity-70">
														by {mod.author}
													</p>
												</div>
											</div>
										</td>
										<td>
											<p class="font-semibold text-sm">{mod.version}</p>
										</td>
										<td>
											<div class="flex items-center justify-end gap-1">
												<button class="btn btn-error btn-sm btn-square">
													<Trash2 size={14} />
												</button>
												<button class="btn btn-sm btn-square">
													<EllipsisVertical size={14} />
												</button>
											</div>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{/if}
				</div>
			</div>
		{/if}
	</div>
</div>

<Modal bind:open={isSettingsModalOpen} title="Instance Settings" class="max-w-2xl">
	<div class="space-y-4">
		<div class="form-control">
			<label for="instance-name" class="label py-0.5">
				<span class="label-text text-sm">Instance Name</span>
			</label>
			<input
				id="instance-name"
				type="text"
				placeholder="Instance Name"
				class="input input-bordered w-full"
				bind:value={editName}
			/>
		</div>

		<div class="form-control">
			<label for="instance-version" class="label py-0.5">
				<span class="label-text text-sm">Game Version</span>
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
			<label for="instance-path" class="label py-0.5">
				<span class="label-text text-sm">Installation Path</span>
			</label>
			<div class="join w-full">
				<input
					id="instance-path"
					type="text"
					placeholder="/path/to/instance"
					class="input input-bordered join-item flex-1 font-mono"
					bind:value={editPath}
				/>
				<button class="btn btn-accent join-item" onclick={handleBrowse}>
					<Folder size={16} />
					Browse
				</button>
			</div>
		</div>

		<div class="form-control">
			<label for="instance-args" class="label py-0.5">
				<span class="label-text text-sm">Launch Arguments</span>
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
						Add
					</button>
				</div>
				{#if editArgs.length > 0}
					<div class="flex flex-wrap gap-1.5">
						{#each editArgs as arg, i (i)}
							<span class="badge badge-ghost badge-neutral gap-1">
								<button
									type="button"
									class="btn btn-ghost btn-xs btn-square p-0 h-4 w-4 min-h-0 text-base-content/60 hover:text-base-content"
									onclick={() => moveArg(i, -1)}
									disabled={i === 0}
								>
									&#8592;
								</button>
								<span class="font-mono text-xs">{arg}</span>
								<button
									type="button"
									class="btn btn-ghost btn-xs btn-square p-0 h-4 w-4 min-h-0 text-base-content/60 hover:text-base-content"
									onclick={() => moveArg(i, 1)}
									disabled={i === editArgs.length - 1}
								>
									&#8594;
								</button>
								<button
									type="button"
									class="btn btn-ghost btn-xs btn-square p-0 h-4 w-4 min-h-0 text-base-content/60 hover:text-base-content"
									onclick={() => removeArg(i)}
								>
									&times;
								</button>
							</span>
						{/each}
					</div>
				{/if}
				<p class="text-xs opacity-60">Space-separated arguments.</p>
			</div>
		</div>

		{#if availablePlatforms.length > 1}
			<div class="form-control">
				<label for="instance-platform" class="label py-0.5">
					<span class="label-text text-sm">Platform</span>
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
	</div>

	{#snippet actions()}
		<div class="flex justify-end gap-2 w-full">
			<button class="btn btn-ghost" onclick={() => (isSettingsModalOpen = false)}>
				Cancel
			</button>
			<button class="btn btn-accent" onclick={saveSettings}> Save Changes </button>
		</div>
	{/snippet}
</Modal>
