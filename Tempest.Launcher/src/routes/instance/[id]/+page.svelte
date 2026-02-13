<script lang="ts">
	import { page } from "$app/state";
	import { goto } from "$app/navigation";
	import { instanceMap, updateInstance, removeInstance } from "$lib/stores/instance";
	import { processesList } from "$lib/stores/processes";
	import { createCommand, launchGame, killGame } from "$lib/core";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
	import { revealItemInDir } from "@tauri-apps/plugin-opener";
	import { exists, readDir } from "@tauri-apps/plugin-fs";
	import { path as tauriPath } from "@tauri-apps/api";
	import { homeDir } from "@tauri-apps/api/path";
	import { instancePlatforms, type InstancePlatform } from "$lib/types/instance";
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
		Users,
		Loader2,
		AlertCircle,
	} from "@lucide/svelte";

	type ModItem = {
		id: string;
		name: string;
		author: string;
		version: string;
		enabled: boolean;
		icon?: string;
	};

	let activeTab = $state<"content" | "champions">("content");

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

	if (!instance) {
		goto("/library");
	}

	let isSettingsModalOpen = $state(false);
	let editName = $state("");
	let editVersion = $state("");
	let editPath = $state("");
	let editPlatform = $state<InstancePlatform>("Win64");
	let availablePlatforms = $state<InstancePlatform[]>([]);
	let isDetectingPlatforms = $state(false);
	let isLoadingChampions = $state(false);
	let championsError = $state("");
	let champions = $state<string[]>([]);
	let hasLoadedChampions = $state(false);

	const platformCache = new Map<string, InstancePlatform[]>();

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

	async function detectAvailablePlatforms(instancePath: string): Promise<InstancePlatform[]> {
		const cached = platformCache.get(instancePath);
		if (cached) return cached;

		const platformRoots = [
			await tauriPath.join(instancePath, "ChaosGame", "Binaries"),
			await tauriPath.join(instancePath, "Binaries"),
		];

		const detected: InstancePlatform[] = [];
		for (const platform of instancePlatforms) {
			const hasBinaries = await hasFilesInAnyDir(
				platformRoots.map((root) => tauriPath.join(root, platform)),
			);
			if (hasBinaries) {
				detected.push(platform);
			}
		}

		const resolved = detected.length ? detected : [...instancePlatforms];
		platformCache.set(instancePath, resolved);
		return resolved;
	}

	async function hasFilesInAnyDir(directoryPaths: Promise<string>[]): Promise<boolean> {
		for (const dirPromise of directoryPaths) {
			const dirPath = await dirPromise;
			if (await hasFilesInDir(dirPath)) return true;
		}
		return false;
	}

	async function hasFilesInDir(directoryPath: string): Promise<boolean> {
		try {
			if (!(await exists(directoryPath))) return false;
			const entries = await readDir(directoryPath);
			return entries.length > 0;
		} catch {
			return false;
		}
	}

	$effect(() => {
		async function updatePlatforms() {
			if (!editPath) {
				availablePlatforms = [];
				return;
			}
			isDetectingPlatforms = true;
			try {
				availablePlatforms = await detectAvailablePlatforms(editPath);
			} finally {
				isDetectingPlatforms = false;
			}
		}
		updatePlatforms();
	});

	$effect(() => {
		if (!availablePlatforms.length) return;
		if (!availablePlatforms.includes(editPlatform)) {
			editPlatform = availablePlatforms[0] ?? "Win64";
		}
	});

	function extractChampionNames(payload: unknown): string[] {
		if (!payload || typeof payload !== "object") return [];
		const rows = (payload as { Rows?: Record<string, Record<string, unknown>> }).Rows;
		if (!rows || typeof rows !== "object") return [];

		const names = new Set<string>();
		for (const row of Object.values(rows)) {
			if (!row || typeof row !== "object") continue;
			for (const [key, value] of Object.entries(row)) {
				if (!value || typeof value !== "object") continue;
				const fieldValue = (value as { Value?: unknown }).Value;
				if (typeof fieldValue !== "string") continue;

				const lowerKey = key.toLowerCase();
				const isNameKey =
					lowerKey === "name" ||
					lowerKey.endsWith("name") ||
					lowerKey.includes("champion") ||
					lowerKey.includes("god");
				if (!isNameKey) continue;

				const candidate = fieldValue.trim();
				if (!candidate) continue;
				if (candidate.length < 2 || candidate.length > 40) continue;
				if (!/^[A-Za-z][A-Za-z'\-\s]+$/.test(candidate)) continue;

				names.add(candidate);
			}
		}

		return [...names].sort((a, b) => a.localeCompare(b));
	}

	async function loadChampions() {
		if (!instance || isLoadingChampions) return;
		isLoadingChampions = true;
		championsError = "";
		champions = [];

		try {
			const tempestHome = await homeDir();
			const tokensRoot = await tauriPath.join(
				tempestHome,
				".tempest",
				"instances",
				instance.id,
			);
			const fieldsPath = await tauriPath.join(tokensRoot, "fields.dat");
			const functionsPath = await tauriPath.join(tokensRoot, "functions.dat");

			const rootCandidates = [
				instance.path,
				await tauriPath.dirname(instance.path),
				await tauriPath.dirname(await tauriPath.dirname(instance.path)),
			];

			let lastError = "";

			const result = await createCommand([
				"marshal",
				"deserialize",
				{
					"--fields": fieldsPath,
					"--functions": functionsPath,
					"--path": await tauriPath.join(
						instance.path,
						"ChaosGame/CookedPCConsole/assembly.dat",
					),
					"--version": "Legacy",
				},
			]).execute();

			if (result.code === 0 && result.stdout) {
				const payload = JSON.parse(result.stdout) as unknown;
				const extracted = extractChampionNames(payload);
				if (extracted.length) {
					champions = extracted;
				}
			}

			lastError = result.stderr || result.stdout || "No data returned.";

			console.error(lastError);

			if (!champions.length) {
				championsError = lastError || "No champions found in marshal output.";
			}
		} catch (error) {
			championsError = String(error);
		} finally {
			isLoadingChampions = false;
			hasLoadedChampions = true;
		}
	}

	$effect(() => {
		if (activeTab !== "champions") return;
		if (hasLoadedChampions || isLoadingChampions) return;
		loadChampions();
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
						onclick={() => (isRunning ? killGame(instance) : launchGame(instance))}
					>
						{#if isRunning}
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
				<button
					role="tab"
					class={activeTab === "champions" ? "tab tab-active" : "tab"}
					onclick={() => (activeTab = "champions")}
				>
					Champions
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
		{#if activeTab === "champions"}
			<div class="flex-1 overflow-y-auto">
				<div class="px-4 py-6 space-y-4">
					<div class="flex items-center justify-between gap-3">
						<div class="flex items-center gap-3">
							<div
								class="w-10 h-10 rounded-lg bg-base-200 flex items-center justify-center"
							>
								<Users size={20} class="opacity-60" />
							</div>
							<div>
								<h2 class="text-lg font-bold">Champions</h2>
								<p class="text-xs opacity-70">Loaded from marshal data (Legacy)</p>
							</div>
						</div>
						<button class="btn btn-accent btn-sm" onclick={loadChampions}>
							{#if isLoadingChampions}
								<Loader2 size={14} class="animate-spin" />
								Loading...
							{:else}
								Reload
							{/if}
						</button>
					</div>

					{#if isLoadingChampions}
						<div class="alert">
							<Loader2 size={16} class="animate-spin" />
							<span>Deserializing marshal data...</span>
						</div>
					{:else if championsError}
						<div class="alert alert-error">
							<AlertCircle size={16} />
							<span>{championsError}</span>
						</div>
					{:else if champions.length === 0}
						<div class="alert">
							<span>No champions found.</span>
						</div>
					{:else}
						<div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-3">
							{#each champions as champion}
								<div class="card bg-base-200">
									<div class="card-body p-3">
										<div class="flex items-center gap-2">
											<div
												class="w-8 h-8 rounded-md bg-base-300 flex items-center justify-center"
											>
												<Box size={14} class="opacity-60" />
											</div>
											<span class="font-semibold text-sm">{champion}</span>
										</div>
									</div>
								</div>
							{/each}
						</div>
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
