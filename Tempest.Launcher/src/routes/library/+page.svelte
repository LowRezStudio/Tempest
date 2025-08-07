<script lang="ts">
	import Button from "$lib/components/ui/Button.svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import Input from "$lib/components/ui/Input.svelte";
	import Icon from "@iconify/svelte";
	import plus from "@iconify-icons/mdi/plus";
	import cross from "@iconify-icons/mdi/close";
	import code from "@iconify-icons/mdi/code-tags";
	import checkDownload from "@iconify-icons/mdi/access-point-check";
	import checkFolder from "@iconify-icons/mdi/folder-check-outline";
	import folder from "@iconify-icons/mdi/folder-open-outline";
	import info from "@iconify-icons/mdi/information-outline";
	import { onMount } from "svelte";

	let isLibraryEmpty: boolean = $state(true);
	let showModal: boolean = $state(false);
	let showAdvancedSettings: boolean = $state(false);
	let canCreateInstance: boolean = $state(false);
	let selectedImportType: string = $state("download");


	onMount(() => {});

	const setImportType = (importType: string) => {
		selectedImportType = importType;
	};

	const toggleAdvancedSettings = () => {
		showAdvancedSettings = !showAdvancedSettings;
	};

	const addInstance = () => {
		showModal = true;
	};

	const closeModal = () => {
		showModal = false;
	};
</script>

{#if isLibraryEmpty}
	<div class="empty-library">
		<h1>Nothing here...</h1>
		<p>But you can get started by adding an instance of Paladins.</p>
		<Button onclick={addInstance} icon={plus}>Add Instance</Button>
	</div>
{:else}{/if}

<Modal bind:showModal={showModal} header="Adding an instance">
	<div class="content">
		<div class="import-type">
			<div class="button-row">
				<Button selected={selectedImportType == "download"} icon={selectedImportType == "download" ? checkDownload : ""} onclick={() => setImportType("download")} style="height:40px">Download</Button>
				<Button selected={selectedImportType == "disk"} icon={selectedImportType == "disk" ? checkFolder : ""} onclick={() => setImportType("disk")} style="height:40px">From Disk</Button>
			</div>
			<hr>
		</div>
		{#if selectedImportType == "download"}
			<div class="instance-settings">
				<div class="instance-setting">
					<label for="instance-name-input">Name</label>
					<Input id="instance-name-input" autocomplete="off"></Input>
				</div>
				<div class="instance-setting">
					<label for="instance-game-version-dropdown">Game version</label>
					<Input id="instance-game-version-dropdown" autocomplete="off"></Input>
				</div>
				{#if showAdvancedSettings}
					<div class="instance-setting">
						<label for="instance-path">Installation path</label>
						<Input id="instance-path" autocomplete="off"></Input>
					</div>
				{/if}
			</div>
			<div class="creation">
				<div class="button-row-creation">
					<Button onclick={toggleAdvancedSettings} icon={code} style="height:40px">{showAdvancedSettings ? "Hide advanced" : "Show advanced"}</Button>
					<Button onclick={closeModal} icon={cross} style="height:40px">Cancel</Button>
					<Button disabled={!canCreateInstance} icon={plus} style="height:40px">Create</Button>
				</div>
			</div>
		{:else}
			<div class="instance-settings-from-disk">
				<div class="zebi">
					<Button icon={folder} style="height:40px">Import from folder</Button>
				</div>
				<div class="drag-and-drop">
					<Icon icon={info} style="font-size: 20px;"></Icon> Or drag and drop your folder
				</div>
			</div>
		{/if}
	</div>
</Modal>

<style>
	.empty-library {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		text-align: center;
		gap: var(--spacing-sm);
		height: 100%;
		width: 100%;
	}
	.import-type {
		display: flex;
		flex-direction: column;
		padding: 1.5rem 1rem 1rem 1rem;
		gap: 1rem;
	}
	.button-row {
		display: flex;
		justify-content: center;
		align-items: center;
		gap: 0.5rem;
	}
	.import-type > hr {
		height: 2px;
		background-color: rgb(34, 35, 41);
		border: none;
	}
	.instance-settings {
		display: flex;
		flex-direction: column;
		padding: 1rem 1.75rem;
		gap: 2rem;
		width: 70%;
		font-weight: 600;
	}
	.instance-setting {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
	}
	.creation {
		display: flex;
		flex-direction: column;
	}
	.instance-settings-from-disk {
		display: flex;
		flex-direction: column;
		padding: 1rem 1.75rem;
		gap: 0.5rem;
	}
	.drag-and-drop {
		display: flex;
		justify-content: start;
		align-items: center;
		gap: 0.5rem;
		color: rgb(144, 152, 161);
		font-weight: 525;
	}
	.button-row-creation {
		display: flex;
		justify-content: right;
		gap: 0.5rem;
		padding: 0 1.75rem 1rem 1.75rem;
	}
</style>
