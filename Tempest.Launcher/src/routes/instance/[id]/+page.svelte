<script lang="ts">
	import { page } from "$app/state";
	import { goto } from "$app/navigation";
	import { instanceMap, updateInstance, removeInstance } from "$lib/stores/instance";
	import { setupInstance } from "$lib/platforms/setup";
	import { processesList } from "$lib/stores/processes";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
	import { revealItemInDir } from "@tauri-apps/plugin-opener";
	import type { Instance, InstancePlatform } from "$lib/types/instance";
	import {
		Play,
		Settings,
		EllipsisVertical,
		Gamepad2,
		RefreshCw,
		Trash2,
		Box,
		Square,
		Folder,
		FolderOpen,
		Tag,
		History,
	} from "@lucide/svelte";
	import { createInstancePlatformsQuery } from "$lib/queries/instance";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";

	type ModItem = {
		id: string;
		name: string;
		author: string;
		version: string;
		enabled: boolean;
		icon?: string;
	};

	let activeTab = $state<"content">("content");

	// Mock data
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
	let isSettingUp = $derived((instance?.state as { type?: string } | undefined)?.type === "setup");

	if (!instance) {
		goto("/library");
	}

	let isSettingsModalOpen = $state(false);
	let editName = $state("");
	let editVersion = $state("");
	let editPath = $state("");
	let editPlatform = $state<InstancePlatform>("Win64");

	function openSettings() {
		if (!instance) return;
		editName = instance.label;
		editVersion = instance.version || "";
		editPath = instance.path;
		editPlatform = instance.launchOptions?.platform ?? "Win64";
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
			},
		});
		isSettingsModalOpen = false;
	}

	const runSetup = async (targetInstance: Instance) => {
		updateInstance(targetInstance.id, {
			state: { type: "setup" } as unknown as Instance["state"],
		});
		try {
			await setupInstance(targetInstance);
		} catch (error) {
			console.error("Instance setup failed:", error);
		} finally {
			updateInstance(targetInstance.id, {
				state: { type: "prepared" } as unknown as Instance["state"],
			});
		}
	};

	function handleRunSetup() {
		if (!instance || isSettingUp) return;
		void runSetup(instance);
	}

	async function openFolder() {
		if (!instance?.path) return;
		await revealItemInDir(instance.path);
	}

	async function deleteInstance() {
		if (!instance) return;

		const confirmed = await ask(`Are you sure you want to delete "${instance.label}"?`, {
			title: "Delete Instance",
			kind: "warning",
		});

		if (confirmed) {
			removeInstance(instance.id);
			goto("/library");
		}
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
	<div class="bg-base-200">
		<div class="px-4 py-3">
			<div class="flex items-center justify-between">
				<!-- Left: Icon and Info -->
				<div class="flex items-center gap-3">
					<div
						class="w-16 h-16 rounded-xl bg-base-300 flex items-center justify-center shrink-0"
					>
						<Box size={32} class="opacity-60" />
					</div>
					<div>
						<h1 class="text-2xl font-bold mb-1">
							{instance?.label || "Loading..."}
						</h1>
						<div class="flex items-center gap-3 text-sm text-base-content/70">
							<div class="flex items-center gap-1.5">
								<Gamepad2 size={14} />
								<span>{instance?.version || "Unknown version"}</span>
								{#if isSettingUp}
									<div class="tooltip" data-tip="Setting up instance...">
										<span class="loading loading-spinner loading-xs"></span>
									</div>
								{/if}
							</div>
						</div>
					</div>
				</div>

				<!-- Right: Action Buttons -->
				<div class="flex items-center gap-2">
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
					<div class="dropdown dropdown-end">
						<div tabindex="0" role="button" class="btn btn-square">
							<EllipsisVertical size={16} />
						</div>
						<ul
							tabindex="0"
							class="dropdown-content menu bg-base-300 rounded-box z-1 w-52 p-2 shadow-sm"
						>
							<li>
								<button onclick={handleRunSetup} disabled={isSettingUp}>
									<RefreshCw size={16} />
									Run Setup
								</button>
							</li>
							<li>
								<button onclick={openFolder}>
									<FolderOpen size={16} />
									Browse Folder
								</button>
							</li>
							<li>
								<button class="text-error" onclick={deleteInstance}>
									<Trash2 size={16} />
									Delete Instance
								</button>
							</li>
						</ul>
					</div>
				</div>
			</div>
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
		</div>

		<!-- Tabs -->
		<div class="px-4">
			<div role="tablist" class="tabs tabs-border">
				<button
					role="tab"
					class={activeTab === "content" ? "tab tab-active" : "tab"}
					onclick={() => (activeTab = "content")}
				>
					Content
				</button>
			</div>
		</div>
	</div>

	<!-- Content Area -->
	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		{#if activeTab === "content"}
			<!-- Mod List -->
			<div class="flex-1 overflow-y-auto">
				<div class="px-4">
					<table class="table">
						<thead>
							<tr>
								<!-- <th class="w-12">
									<input type="checkbox" class="checkbox checkbox-xs" />
								</th> -->
								<th>
									<button class="flex items-center gap-1 font-semibold text-sm">
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
									<!-- <td>
										<input type="checkbox" class="checkbox checkbox-xs" />
									</td> -->
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
												<p class="text-xs opacity-70">by {mod.author}</p>
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
