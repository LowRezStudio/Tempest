<script lang="ts">
	import Button from "$lib/components/ui/Button.svelte";
	import Dialog from "$lib/components/ui/Dialog.svelte";
	import Input from "$lib/components/ui/Input.svelte";
	import PathPicker from "$lib/components/ui/PathPicker.svelte";
	import Select from "$lib/components/ui/Select.svelte";
	import versions from "$lib/data/versions.json";
	import { addInstance, defaultInstancePath } from "$lib/state/instances.svelte";
	import { CloudDownload, Code, Folder, FolderPlus, Info, Plus, X } from "@lucide/svelte";
	import { path } from "@tauri-apps/api";

	let open = $state(false);

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
	let defaultPath = $state("");

	const canAddInstance = $derived(selectedVersionValue != undefined);
	const selectedVersion = $derived(Object.values(versions).flat().find((item) => item.id === selectedVersionValue));

	const addInstanceFromFolder = (path: string) => {};

	const addInstanceFromDepot = () => {
		addInstance({ label: selectedName, path: selectedPath, version: selectedVersion?.version });
		open = false;
	};

	$effect(() => {
		path.join(
			defaultInstancePath.current,
			selectedName.length == 0 ? selectedVersion?.version ?? "Paladins" : selectedName,
		)
			.then((res) => defaultPath = res);
	});
</script>

<button
	onclick={() => (open = !open)}
	class="grid place-items-center size-10 rounded-xl text-gray-400 transition-all duration-300 ease-in-out transform translate-z-0 hover:text-primary-300 hover:scale-102 active:scale-98 cursor-pointer"
>
	<Plus class="size-5 transition-transform duration-200 ease-in-out" />
</button>

<Dialog bind:open title="Adding an instance">
	<div class="flex flex-col gap-4 items-center">
		<div class="flex items-center justify-center gap-2">
			<Button
				onclick={() => (selectedImportType = "download")}
				kind={selectedImportType == "download" ? "accented" : "normal"}
				icon={selectedImportType == "download" ? CloudDownload : undefined}
			>Download</Button>
			<Button
				onclick={() => (selectedImportType = "folder")}
				kind={selectedImportType == "folder" ? "accented" : "normal"}
				icon={selectedImportType == "folder" ? Folder : undefined}
			>From Folder</Button>
		</div>
		<hr class="border-background-700 w-full" />
	</div>
	{#if selectedImportType == "download"}
		<div class="flex flex-col gap-4 py-4">
			<div class="flex flex-col gap-2">
				<p>Name</p>
				<Input bind:value={selectedName} placeholder={selectedVersion?.version}></Input>
			</div>
			<div class="flex flex-col gap-2">
				<p>Game version</p>
				<Select
					type="single"
					items={versionItems}
					bind:value={selectedVersionValue}
					placeholder="Choose a version"
				/>
			</div>
			{#if showAdvanced}
				<div class="flex flex-col gap-2">
					<p>Installation path</p>
					<!-- TODO: Remove the fallback value once selectedPath has a default value -->
					<PathPicker
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
				icon={Plus}
				disabled={!canAddInstance}
			>Download</Button>
		</div>
	{:else}
		<div class="flex flex-col gap-4 pt-4">
			<div class="">
				<PathPicker
					bind:value={selectedPath}
					type="folder"
					multiple={false}
					placeholder="Import from Folder"
				></PathPicker>
			</div>
			<div class="flex gap-2 items-center">
				<Info></Info>
				<p>Or drag and drop your folder.</p>
			</div>
		</div>
	{/if}
</Dialog>
