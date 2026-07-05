<script lang="ts">
	import { AlertCircle, BookOpen, CloudDownload, Code, Folder, Loader2 } from "@lucide/svelte";
	import { Tabs } from "bits-ui";
	import { path } from "@tauri-apps/api";
	import { resolveResource } from "@tauri-apps/api/path";
	import { open as openDialog } from "@tauri-apps/plugin-dialog";
	import { openUrl } from "@tauri-apps/plugin-opener";
	import { platform } from "@tauri-apps/plugin-os";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { installMod } from "$lib/core/mods";
	import versions from "$lib/data/versions.json";
	import { m } from "$lib/paraglide/messages";
	import { createIdentifyBuildMutation } from "$lib/queries/core";
	import {
		createDefaultInstancePathQuery,
		createSetupInstanceMutation,
	} from "$lib/queries/instance";
	import {
		RIGBY_BASE_URL,
		RIGBY_MANIFEST_URL_TEMPLATE,
		WIKI_BASE_URL,
	} from "$lib/rigby/constants";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { addInstance, updateInstance } from "$lib/stores/instance.svelte";
	import { defaultInstancePath } from "$lib/stores/settings.svelte";
	import type { Instance, InstanceState } from "$lib/types/instance";

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
	let enableConsole = $state(true);
	let copyStatus = $state<"idle" | "copied" | "failed">("idle");

	let detectionError = $state("");
	let hasDetected = $state(false);

	const selectedVersion = $derived(flatVersions.find((v) => v.id === selectedVersionId));
	const selectedAppId = $derived(selectedVersion?.appId ?? 444090);
	const isPts = $derived(
		(selectedAppId !== 444090 && selectedAppId !== 44090) ||
			!!selectedVersion?.name?.toLowerCase().includes("pts"),
	);
	const selectedDepotId = $derived(isPts ? 596351 : 444091);

	const supportsCloudDownload = $derived(!!selectedVersion?.version);

	const isValid = $derived(
		selectedTab === "download" ? !!selectedVersionId : !!(selectedVersionId && selectedPath),
	);

	async function handleBrowse() {
		const result = await openDialog({
			directory: true,
			multiple: false,
			title: m.wizard_select_installation_folder(),
			defaultPath: defaultInstancePath.value || undefined,
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
					hasDetected = true;
				} else {
					detectionError = m.wizard_build_not_in_database({
						patchName: info.PatchName,
						versionGroup: info.VersionGroup,
					});
					hasDetected = true;
				}
			} else {
				detectionError = m.wizard_could_not_identify();
			}
		} catch (error) {
			console.error("Detection error:", error);
			detectionError = m.wizard_identify_error();
		}
	}

	async function getInstancePath() {
		if (selectedPath) return selectedPath;
		if (!selectedVersion?.version) return "";
		if (defaultInstancePath.value) {
			return await path.join(defaultInstancePath.value, selectedVersion.version);
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

		if (selectedTab === "download") {
			if (!selectedVersion?.version || !supportsCloudDownload) return;

			const newInstance: Instance = {
				id: crypto.randomUUID(),
				label: selectedName || selectedVersion.name || selectedVersion.version,
				version: selectedVersion.version,
				manifestId: selectedVersion.id,
				appId: selectedAppId,
				path: instancePath,
				launchOptions: {
					dllList: [],
					args: [],
					noDefaultArgs: false,
					log: false,
				},
				state: { type: "downloading" },
			};

			addInstance(newInstance);

			restoreQueue.add({
				manifests: [RIGBY_MANIFEST_URL_TEMPLATE.replace("{id}", selectedVersion.id)],
				outDir: instancePath,
				baseUrl: RIGBY_BASE_URL,
			});

			open = false;
			return;
		}

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
				m.wizard_paladins_instance(),
			version: selectedVersion?.version,
			manifestId: selectedVersion?.id,
			appId: selectedAppId,
			path: instancePath,
			launchOptions: {
				dllList: [],
				args: [],
				noDefaultArgs: false,
				log: false,
			},
			state: {
				type: "setup",
			},
		};

		addInstance(newInstance);
		void runSetup(newInstance);

		if (enableConsole) {
			try {
				const modFile = await resolveResource("Tempest Mod.tempest");
				await installMod(instancePath, modFile, true, true);
			} catch (error) {
				console.error("Failed to install Console mod:", error);
			}
		}

		open = false;
	}

	$effect(() => {
		if (!open) {
			selectedTab = "download";
			selectedName = "";
			selectedVersionId = "";
			selectedPath = "";
			showAdvanced = false;
			enableConsole = true;
			hasDetected = false;
			detectionError = "";
		}
	});

	const defaultPathQuery = createDefaultInstancePathQuery(
		() => selectedVersion?.version,
		() => defaultInstancePath.value,
	);
	const setupInstanceMutation = createSetupInstanceMutation();

	let defaultPathPlaceholder = $derived(defaultPathQuery.data ?? "");
	const depotDownloaderBinary = $derived(
		platform() === "windows" ? ".\\DepotDownloader.exe" : "./DepotDownloader",
	);
	const downloadPathForCommand = $derived(
		selectedPath || defaultPathPlaceholder || "<install-path>",
	);
	const depotDownloaderCommand = $derived(
		selectedVersionId ?
			`${depotDownloaderBinary} -app ${selectedAppId} -depot ${selectedDepotId} -manifest ${selectedVersionId} -os windows -dir "${downloadPathForCommand}" -qr -remember-password`
		:	"",
	);

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

	async function handleOpenWiki() {
		const ref = selectedVersion?.wikiReference;
		if (!ref) return;
		try {
			await openUrl(`${WIKI_BASE_URL}${encodeURIComponent(ref)}?fandom=allow`);
		} catch (error) {
			console.error("Failed to open wiki:", error);
		}
	}
</script>

<Modal bind:open title={m.wizard_title()} class="max-w-2xl" onsubmit={handleCreate}>
	<Tabs.Root bind:value={selectedTab} class="w-full mb-4">
		<Tabs.List class="tabs tabs-border w-full">
			<Tabs.Trigger
				value="download"
				class="tab gap-2 flex-1 data-[state=active]:tab-active"
			>
				<CloudDownload size={16} />
				<span>{m.wizard_download()}</span>
			</Tabs.Trigger>
			<Tabs.Trigger
				value="folder"
				class="tab gap-2 flex-1 data-[state=active]:tab-active"
			>
				<Folder size={16} />
				<span>{m.wizard_import_existing()}</span>
			</Tabs.Trigger>
		</Tabs.List>
	</Tabs.Root>

	<div class="space-y-4">
		<div class="alert">
			<div class="flex gap-2 items-start">
				{#if selectedTab === "download"}
					<CloudDownload size={16} class="mt-0.5 shrink-0" />
					<div>
						<h4 class="font-semibold text-sm">{m.wizard_download_description()}</h4>
						<p class="text-xs opacity-80">
							{m.wizard_download_hint()}
						</p>
					</div>
				{:else}
					<Folder size={16} class="mt-0.5 shrink-0" />
					<div>
						<h4 class="font-semibold text-sm">{m.wizard_import_description()}</h4>
						<p class="text-xs opacity-80">
							{m.wizard_import_hint()}
						</p>
					</div>
				{/if}
			</div>
		</div>

		<div class="space-y-4">
			{#if selectedTab === "download" || (selectedTab === "folder" && (hasDetected || !!detectionError))}
				<div class="form-control">
					<label for="game-version" class="label py-0.5">
						<span class="label-text text-sm">{m.wizard_game_version()}</span>
					</label>
					<div class="flex gap-2">
						<select
							id="game-version"
							class="select select-bordered flex-1"
							bind:value={selectedVersionId}
						>
							<option value="" disabled>{m.wizard_select_version()}</option>
							{#each versionOptions as version (version.value)}
								<option value={version.value}>{version.label}</option>
							{/each}
						</select>
						{#if selectedVersion?.wikiReference}
							<button
								type="button"
								class="btn btn-square btn-ghost"
								title={m.wizard_open_wiki()}
								aria-label={m.wizard_open_wiki()}
								onclick={handleOpenWiki}
							>
								<BookOpen size={16} />
							</button>
						{/if}
					</div>
					{#if selectedTab === "download" && selectedVersionId}
						<div class="mt-2 space-y-1.5">
							<label class="label py-0.5 justify-between">
								<span class="label-text text-sm"
									>{m.wizard_depot_downloader_command()}</span
								>
								<button
									class="btn btn-accent btn-xs"
									type="button"
									onclick={handleCopyCommand}
								>
									{copyStatus === "copied" ? m.common_copied()
									: copyStatus === "failed" ? m.common_copy_failed()
									: m.common_copy()}
								</button>
							</label>
							<div
								class="rounded-sm bg-base-200 border border-base-300 px-3 py-2 text-xs font-mono whitespace-pre-wrap select-text"
								style="user-select: text;"
							>
								{depotDownloaderCommand}
							</div>
						</div>
					{/if}
				</div>
			{/if}

			{#if selectedTab === "folder"}
				<div class="form-control">
					<label for="folder-path" class="label py-0.5">
						<span class="label-text text-sm">{m.wizard_installation_path()}</span>
					</label>
					<div class="join w-full">
						<input
							id="folder-path"
							type="text"
							placeholder={defaultPathPlaceholder}
							class="input input-bordered join-item flex-1 font-mono"
							bind:value={selectedPath}
						/>
						<button
							class="btn btn-accent join-item"
							type="button"
							onclick={handleBrowse}
						>
							<Folder size={16} />
							{m.common_browse()}
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
				<div class="divider my-2 text-xs">{m.wizard_advanced_options()}</div>

				<div class="form-control">
					<label for="instance-name" class="label py-0.5">
						<span class="label-text text-sm">{m.wizard_instance_name()}</span>
						<span class="label-text-alt text-xs">{m.common_optional()}</span>
					</label>
					<input
						id="instance-name"
						type="text"
						placeholder={selectedVersion?.name ||
							selectedVersion?.version ||
							m.wizard_my_custom_instance()}
						class="input input-bordered w-full"
						bind:value={selectedName}
					/>
					<div class="label py-0.5">
						<span class="label-text-alt text-xs">{m.wizard_leave_empty_version()}</span>
					</div>
				</div>

				{#if selectedTab === "download"}
					<div class="form-control">
						<label for="download-path" class="label py-0.5">
							<span class="label-text text-sm">{m.wizard_installation_path()}</span>
							<span class="label-text-alt text-xs">{m.common_optional()}</span>
						</label>
						<div class="join w-full">
							<input
								id="download-path"
								type="text"
								placeholder={defaultPathPlaceholder}
								class="input input-bordered join-item flex-1 font-mono"
								bind:value={selectedPath}
							/>
							<button
								class="btn btn-accent join-item"
								type="button"
								onclick={handleBrowse}
							>
								<Folder size={16} />
								{m.common_browse()}
							</button>
						</div>
						<div class="label py-0.5">
							<span class="label-text-alt text-xs"></span>
						</div>
					</div>
				{/if}

				<div class="form-control">
					<label class="label cursor-pointer justify-start gap-3 py-1">
						<input type="checkbox" class="checkbox checkbox-accent checkbox-sm" bind:checked={enableConsole} />
						<span class="label-text text-sm">Enable Console</span>
					</label>
				</div>
			{/if}
		</div>
	</div>

	{#snippet actions()}
		<div class="flex items-center justify-between w-full">
			<button
				class="btn btn-ghost"
				type="button"
				onclick={() => (showAdvanced = !showAdvanced)}
			>
				<Code size={16} />
				{showAdvanced ? m.wizard_hide_advanced() : m.wizard_advanced_options()}
			</button>
			<div class="flex gap-2">
				<button class="btn btn-ghost" type="button" onclick={() => (open = false)}
					>{m.common_cancel()}</button
				>
				<button
					class="btn btn-accent"
					type="submit"
					disabled={!isValid ||
						isDetecting ||
						(selectedTab === "download" && !supportsCloudDownload)}
				>
					{#if selectedTab === "download"}
						<CloudDownload size={16} />
						{supportsCloudDownload ?
							m.wizard_download_and_create()
						:	m.wizard_not_available()}
					{:else if isDetecting}
						<Loader2 size={16} class="animate-spin" />
						{m.common_identifying()}
					{:else}
						<Folder size={16} />
						{m.wizard_import_instance()}
					{/if}
				</button>
			</div>
		</div>
	{/snippet}
</Modal>
