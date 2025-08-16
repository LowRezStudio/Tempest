<script lang="ts">
	import Button from "$lib/components/ui/Button.svelte";
	import Dialog from "$lib/components/ui/Dialog.svelte";
	import Input from "$lib/components/ui/Input.svelte";
	import PathPicker from "$lib/components/ui/PathPicker.svelte";
	import Select from "$lib/components/ui/Select.svelte";
	import versions from "$lib/data/versions.json";
	import { addInstance, defaultInstancePath } from "$lib/state/instances.svelte";
	import { CloudDownload, Code, Download, Folder, Plus, X } from "@lucide/svelte";
	import { path } from "@tauri-apps/api";

	interface Props {
		open?: boolean;
	}
	let { open = $bindable(false) }: Props = $props();

	const versionItems = Object.entries(versions).flatMap(([group, list]) =>
		list.map((item) => ({
			value: item.id,
			label: `${item.version} - ${item.date}`,
			group: group,
		}))
	);

	let selectedImportType = $state("download");
	let selectedName = $state("");
	let selectedVersionValue = $state<string | undefined>();
	let showAdvanced = $state(false);
	let selectedPath = $state("");
	let defaultName = $state("");
	let defaultPath = $state("");

	const flatVersions = Object.values(versions).flat();
	const selectedVersion = $derived(flatVersions.find((item) => item.id === selectedVersionValue));

	const addInstanceFromFolder = async () => {
		addInstance({
			label: selectedName.length == 0 ? defaultName : selectedName,
			path: selectedPath,
			version: selectedVersion?.version,
			state: {
				type: "prepared",
			},
		});

		open = false;
	};

	const addInstanceFromDepot = () => {
		// TO-DO: ACTUALLY DOWNLOAD IT
		addInstance({
			label: selectedName.length == 0 ? defaultName : selectedName,
			path: selectedPath.length == 0 ? defaultPath : selectedPath,
			version: selectedVersion?.version,
			state: {
				type: "unprepared",
				status: "paused",
				percentage: 0,
			},
		});

		open = false;
	};

	const switchTab = (importType: string) => {
		if (selectedImportType == importType) return;

		reset();
		selectedImportType = importType;
	};

	const reset = () => {
		selectedImportType = "download";
		selectedName = "";
		selectedVersionValue = undefined;
		showAdvanced = false;
		selectedPath = "";
	};

	// tauri path apis are async so yeah, sorry
	$effect(() => {
		path
			.join(
				defaultInstancePath.current,
				selectedName.length == 0 ? selectedVersion?.version ?? "Paladins" : selectedName,
			)
			.then((res) => (defaultPath = res));

		path.basename(selectedPath.length == 0 ? defaultPath : selectedPath)
			.then((res) => (defaultName = res));
	});

	// reset state when reopening the modal
	$effect(() => open ? reset() : undefined);
</script>

<Dialog bind:open title="Adding an instance">
	<div class="flex flex-col gap-4 items-center">
		<div class="flex items-center justify-center gap-2">
			<Button
				onclick={() => switchTab("download")}
				kind={selectedImportType == "download" ? "accented" : "normal"}
				icon={selectedImportType == "download" ? CloudDownload : undefined}
			>Download</Button>
			<Button
				onclick={() => switchTab("folder")}
				kind={selectedImportType == "folder" ? "accented" : "normal"}
				icon={selectedImportType == "folder" ? Folder : undefined}
			>From Folder</Button>
		</div>
		<hr class="border-background-700 w-full" />
	</div>
	{#if selectedImportType == "download"}
		<div class="flex flex-col gap-4 py-4">
			{#if showAdvanced}
				<div class="flex flex-col gap-1 min-w-0">
					<label class="text-sm" for="name-advanced">Name</label>
					<Input bind:value={selectedName} id="name-advanced" placeholder={defaultName}></Input>
				</div>
			{/if}
			<div class="flex flex-col gap-1 min-w-0">
				<!-- svelte-ignore a11y_label_has_associated_control -->
				<label class="text-sm">Game version</label>
				<Select
					type="single"
					items={versionItems}
					bind:value={selectedVersionValue}
					placeholder="Choose a version"
				/>
			</div>
			{#if showAdvanced}
				<div class="flex flex-col gap-1 min-w-0">
					<label class="text-sm" for="path-install">Installation path</label>
					<PathPicker
						id="path-install"
						bind:value={selectedPath}
						type="folder"
						multiple={false}
						placeholder={defaultPath}
					></PathPicker>
				</div>
			{/if}
		</div>
		<div class="flex items-center justify-end gap-2">
			<Button
				onclick={() => (showAdvanced = !showAdvanced)}
				icon={Code}
			>{showAdvanced ? "Hide advanced" : "Show advanced"}</Button>
			<Button
				onclick={() => (open = false)}
				icon={X}
			>Cancel</Button>
			<Button
				onclick={addInstanceFromDepot}
				kind="accented"
				icon={Download}
				disabled={!selectedVersionValue}
			>Download</Button>
		</div>
	{:else}
		<div class="flex flex-col gap-4 py-4">
			{#if showAdvanced}
				<div class="flex flex-col gap-1 min-w-0">
					<label class="text-sm" for="name">Name</label>
					<Input bind:value={selectedName} id="name" placeholder={defaultName}></Input>
				</div>
			{/if}
			<div class="flex flex-col gap-1 min-w-0">
				<!-- svelte-ignore a11y_label_has_associated_control -->
				<label class="text-sm">Game version</label>
				<Select
					type="single"
					items={versionItems}
					bind:value={selectedVersionValue}
					placeholder="Choose a version"
				/>
			</div>
			<div class="flex flex-col gap-1 min-w-0">
				<label class="text-sm" for="path-import">Path</label>
				<PathPicker
					id="path-import"
					bind:value={selectedPath}
					type="folder"
					multiple={false}
					placeholder="Select path"
				/>
			</div>
		</div>
		<div class="flex items-center justify-end gap-2">
			<Button
				onclick={() => (showAdvanced = !showAdvanced)}
				icon={Code}
			>{showAdvanced ? "Hide advanced" : "Show advanced"}</Button>
			<Button
				onclick={() => (open = false)}
				icon={X}
			>Cancel</Button>
			<Button
				onclick={addInstanceFromFolder}
				kind="accented"
				icon={Plus}
				disabled={!selectedVersionValue || !selectedPath}
			>Import</Button>
		</div>
	{/if}
</Dialog>
