<script lang="ts">
	import Modal from "$lib/components/ui/Modal.svelte";
	import versions from "$lib/data/versions.json";
	import { addInstance, updateInstance } from "$lib/stores/instance";
	import type { Instance, InstanceState } from "$lib/types/instance";
	import { CloudDownload, Folder, Code, Loader2, AlertCircle } from "@lucide/svelte";
	import { open as openDialog } from "@tauri-apps/plugin-dialog";
	import { defaultInstancePath } from "$lib/stores/settings";
	import {
		createDefaultInstancePathQuery,
		createSetupInstanceMutation,
	} from "$lib/queries/instance";
	import { createIdentifyBuildMutation } from "$lib/queries/core";
	import { path } from "@tauri-apps/api";

	interface Props {
		open?: boolean;
	}

	let { open = $bindable(false) }: Props = $props();

	const flatVersions = versions;

	const versionOptions = flatVersions.map((item) => ({
		value: item.id,
		label: `${item.version} - ${item.name} (${item.date.split("T")[0]})`,
		version: item.version,
	}));

	let selectedTab = $state<"download" | "folder">("download");
	let selectedName = $state("");
	let selectedVersionId = $state("");
	let selectedPath = $state("");
	let showAdvanced = $state(false);

	let detectionError = $state("");
	let hasDetected = $state(false);

	const selectedVersion = $derived(flatVersions.find((v) => v.id === selectedVersionId));

	const isValid = $derived(
		selectedTab === "download" ? !!selectedVersionId : (
			!!(hasDetected && selectedVersionId && selectedPath)
		),
	);

	async function handleBrowse() {
		const result = await openDialog({
			directory: true,
			multiple: false,
			title: "Select Paladins Installation Folder",
		});
		if (result) {
			selectedPath = result;
			if (selectedTab === "folder") {
				await performDetection(result);
			}
		}
	}

	const identifyBuildMutation = createIdentifyBuildMutation();
	let isDetecting = $derived(identifyBuildMutation.isPending);

	async function performDetection(path: string) {
		detectionError = "";
		hasDetected = false;
		try {
			const info = await identifyBuildMutation.mutateAsync(path);
			if (info) {
				const version = flatVersions.find((v) => v.id === info.Id);
				if (version) {
					selectedVersionId = version.id;
					if (!selectedName) {
						selectedName = info.VersionGroup;
					}
					hasDetected = true;
				} else {
					detectionError = `Build identified as ${info.PatchName} (${info.VersionGroup}), but it's not in our database.`;
				}
			} else {
				detectionError = "Could not identify build in this folder.";
			}
		} catch (error) {
			console.error("Detection error:", error);
			detectionError = "An error occurred during build identification.";
		}
	}

	async function getInstancePath() {
		if (selectedPath) return selectedPath;
		if (!selectedVersion?.version) return "";
		if ($defaultInstancePath) {
			return await path.join($defaultInstancePath, selectedVersion.version);
		}
		return `/instances/${selectedVersion.version}`;
	}

	const runSetup = async (instance: Instance) => {
		try {
			await setupInstanceMutation.mutateAsync(instance);
		} catch (error) {
			console.error("Instance setup failed:", error);
		} finally {
			updateInstance(instance.id, { state: { type: "prepared" } });
		}
	};

	async function handleCreate() {
		if (!isValid) return;

		const instancePath = await getInstancePath();

		if (selectedTab === "folder" && !selectedVersionId) {
			await performDetection(instancePath);
			if (!selectedVersionId) return;
		}

		const newInstance: Instance = {
			id: crypto.randomUUID(),
			label:
				selectedName ||
				selectedVersion?.name ||
				selectedVersion?.version ||
				"Paladins Instance",
			version: selectedVersion?.version,
			path: instancePath,
			launchOptions: {
				dllList: [],
				args: [],
				noDefaultArgs: false,
				log: false,
			},
			state: {
				type: "setup",
			} as unknown as InstanceState,
		};

		addInstance(newInstance);
		void runSetup(newInstance);

		open = false;
	}

	$effect(() => {
		if (!open) {
			selectedTab = "download";
			selectedName = "";
			selectedVersionId = "";
			selectedPath = "";
			showAdvanced = false;
			hasDetected = false;
			detectionError = "";
		}
	});

	const defaultPathQuery = createDefaultInstancePathQuery(
		() => selectedVersion?.version,
		() => $defaultInstancePath,
	);
	const setupInstanceMutation = createSetupInstanceMutation();

	let defaultPathPlaceholder = $derived(defaultPathQuery.data ?? "");
</script>

<Modal bind:open title="Create New Instance" class="max-w-2xl">
	<div role="tablist" class="tabs tabs-border w-full mb-4">
		<button
			role="tab"
			class={["tab gap-2 flex-1", selectedTab === "download" && "tab-active"]}
			onclick={() => (selectedTab = "download")}
		>
			<CloudDownload size={16} />
			<span>Download</span>
		</button>
		<button
			role="tab"
			class={["tab gap-2 flex-1", selectedTab === "folder" && "tab-active"]}
			onclick={() => (selectedTab = "folder")}
		>
			<Folder size={16} />
			<span>Import Existing</span>
		</button>
	</div>

	<div class="space-y-4">
		<div class="alert">
			<div class="flex gap-2 items-start">
				{#if selectedTab === "download"}
					<CloudDownload size={16} class="mt-0.5 shrink-0" />
					<div>
						<h4 class="font-semibold text-sm">Download a game version</h4>
						<p class="text-xs opacity-80">
							Select a Paladins version to download and install automatically
						</p>
					</div>
				{:else}
					<Folder size={16} class="mt-0.5 shrink-0" />
					<div>
						<h4 class="font-semibold text-sm">Import from existing installation</h4>
						<p class="text-xs opacity-80">
							Point to an existing Paladins installation folder
						</p>
					</div>
				{/if}
			</div>
		</div>

		<div class="space-y-4">
			{#if selectedTab === "download" || (selectedTab === "folder" && hasDetected)}
				<div class="form-control">
					<label for="game-version" class="label py-0.5">
						<span class="label-text text-sm">Game Version</span>
					</label>
					<select
						id="game-version"
						class="select select-bordered w-full"
						bind:value={selectedVersionId}
					>
						<option value="" disabled>Select a version...</option>
						{#each versionOptions as version (version.value)}
							<option value={version.value}>{version.label}</option>
						{/each}
					</select>
				</div>
			{/if}

			{#if selectedTab === "folder"}
				<div class="form-control">
					<label for="folder-path" class="label py-0.5">
						<span class="label-text text-sm">Installation Path</span>
					</label>
					<div class="join w-full">
						<input
							id="folder-path"
							type="text"
							placeholder={defaultPathPlaceholder}
							class="input input-bordered join-item flex-1 font-mono"
							bind:value={selectedPath}
						/>
						<button class="btn btn-accent join-item" onclick={handleBrowse}>
							<Folder size={16} />
							Browse
						</button>
					</div>
					{#if detectionError}
						<div class="label py-1">
							<span class="label-text-alt text-error flex items-center gap-1">
								<AlertCircle size={12} />
								{detectionError}
							</span>
						</div>
					{/if}
				</div>
			{/if}

			{#if showAdvanced}
				<div class="divider my-2 text-xs">Advanced Options</div>

				<div class="form-control">
					<label for="instance-name" class="label py-0.5">
						<span class="label-text text-sm">Instance Name</span>
						<span class="label-text-alt text-xs">Optional</span>
					</label>
					<input
						id="instance-name"
						type="text"
						placeholder={selectedVersion?.name ||
							selectedVersion?.version ||
							"My Custom Instance"}
						class="input input-bordered w-full"
						bind:value={selectedName}
					/>
					<div class="label py-0.5">
						<span class="label-text-alt text-xs">Leave empty to use version name</span>
					</div>
				</div>

				{#if selectedTab === "download"}
					<div class="form-control">
						<label for="download-path" class="label py-0.5">
							<span class="label-text text-sm">Installation Path</span>
							<span class="label-text-alt text-xs">Optional</span>
						</label>
						<div class="join w-full">
							<input
								id="download-path"
								type="text"
								placeholder={defaultPathPlaceholder}
								class="input input-bordered join-item flex-1 font-mono"
								bind:value={selectedPath}
							/>
							<button class="btn btn-accent join-item" onclick={handleBrowse}>
								<Folder size={16} />
								Browse
							</button>
						</div>
						<div class="label py-0.5">
							<span class="label-text-alt text-xs">Defaults to instances folder</span>
						</div>
					</div>
				{/if}
			{/if}
		</div>
	</div>

	{#snippet actions()}
		<div class="flex items-center justify-between w-full">
			<button class="btn btn-ghost" onclick={() => (showAdvanced = !showAdvanced)}>
				<Code size={16} />
				{showAdvanced ? "Hide Advanced" : "Advanced Options"}
			</button>
			<div class="flex gap-2">
				<button class="btn btn-ghost" onclick={() => (open = false)}>Cancel</button>
				<button
					class="btn btn-accent"
					disabled={!isValid || isDetecting}
					onclick={handleCreate}
				>
					{#if selectedTab === "download"}
						<CloudDownload size={16} />
						Download & Create
					{:else if isDetecting}
						<Loader2 size={16} class="animate-spin" />
						Identifying...
					{:else}
						<Folder size={16} />
						Import Instance
					{/if}
				</button>
			</div>
		</div>
	{/snippet}
</Modal>
