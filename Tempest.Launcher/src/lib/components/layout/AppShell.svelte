<script lang="ts">
	import { page } from "$app/state";
	import Button from "../ui/Button.svelte";
	import Input from "../ui/Input.svelte";
	import Select from "../ui/Select.svelte";
	import Selector from "../ui/Selector.svelte";
	import { addInstance, instances } from "$lib/state/instances.svelte";
	import { createDialog, melt } from "@melt-ui/svelte";
	import {
		type Icon,
		House,
		X,
		Code,
		Library,
		Minus,
		Plus,
		Settings,
		Swords,
		Users,
		Check,
		Info,
		Folder,
		CloudDownload,
		FolderPlus,
	} from "@lucide/svelte";
	import { cubicOut } from "svelte/easing";
	import { fade, fly } from "svelte/transition";
	import { render } from "svelte/server";
	import type { Snippet } from "svelte";

	interface Props {
		children?: Snippet;
	}

	let { children }: Props = $props();

	let isTransitioning = $state(false);

	type Page = {
		name: string;
		path: string;
		icon: typeof Icon;
	};

	const pageItems: Page[] = [
		{ name: "Home", path: "/", icon: House },
		{ name: "Library", path: "/library", icon: Library },
		{ name: "Champions", path: "/champions", icon: Swords },
		{ name: "Servers", path: "/servers", icon: Users },
	];

	const {
		elements: {
			trigger,
			overlay,
			content,
			title,
			description,
			close,
			portalled,
		},
		states: { open },
	} = createDialog({
		forceVisible: true,
	});

	let showAdvancedSettings: boolean = $state(false);
	let selectedImportType: string = $state("download");

	let instanceName = $state("");
	let selectedGameVersion = $state("");
	// TODO: SET chosenPath DEFAULT VALUE TO THE DEFAULT INSTALL LOCATION
	let chosenPath = $state("");

	const setImportType = (importType: string) => {
		selectedImportType = importType;
	};

	const toggleAdvancedSettings = () => {
		showAdvancedSettings = !showAdvancedSettings;
	};

	const canAddInstance = (): boolean => {
		return instanceName.length > 0 && selectedGameVersion.length > 0;
	};

	const isActive = (item: { path: string }) => page.url.pathname == item.path;
</script>

{#if $open}
	<div class="" use:melt={$portalled}>
		<div
			use:melt={$overlay}
			class="fixed inset-0 z-50 bg-black/50"
			transition:fade={{ duration: 150 }}
		></div>
		<div
			class="
				fixed left-1/2 top-1/2 z-50 max-h-[85vh] w-[90vw]
				max-w-[550px] -translate-x-1/2 -translate-y-1/2 rounded-xl bg-[#101013] shadow-lg
			"
			use:melt={$content}
		>
			<div class="modal-header p-5 flex justify-between items-center">
				<h2 use:melt={$title} class="m-0 text-lg font-semibold text-white">
					Adding an instance
				</h2>

				<button
					use:melt={$close}
					aria-label="close"
					class=" rounded-full p-2 cursor-pointer text-text-color bg-component-background transition duration-150 hover:brightness-90 focus:shadow-white"
				>
					<X class="size-6" />
				</button>
			</div>
			<hr class="border-[#222329]" />
			<div class="modal-body p-5">
				<div class="instance-type flex flex-col gap-4 items-center pt-2 pb-5">
					<div
						class="instance-type-buttons flex items-center justify-center gap-2"
					>
						<Button
							onclick={() => setImportType("download")}
							kind={selectedImportType == "download" ? "accented" : "normal"}
							icon={selectedImportType == "download"
								? CloudDownload
								: undefined}
							class="text-text-color font-semibold">Download</Button
						>
						<Button
							onclick={() => setImportType("disk")}
							kind={selectedImportType == "disk" ? "accented" : "normal"}
							icon={selectedImportType == "disk" ? Folder : undefined}
							class="text-text-color font-semibold">From Folder</Button
						>
					</div>
					<hr class="border-[#222329] w-2xs" />
				</div>
				{#if selectedImportType == "download"}
					<div class="instance-settings flex flex-col gap-6 w-[70%]">
						<div class="instance-setting flex flex-col gap-1">
							Name
							<Input bind:value={instanceName} id="instance-name"></Input>
						</div>
						<div class="instance-setting flex flex-col gap-1">
							Game version
							<Select
								choices={{
									"Open Beta": [
										"OB 34",
										"OB 35",
										"OB 36",
										"OB 37",
										"OB 38",
										"OB 39",
										"OB 41",
										"OB 42",
										"OB 43",
										"OB 44",
										"OB 46",
										"OB 48",
										"OB 49",
										"OB 50",
										"OB 51",
										"OB 52",
										"OB 53",
										"OB 54",
										"OB 55",
										"OB 56",
										"OB 57",
										"OB 58",
										"OB 60",
										"OB 61",
										"OB 63",
										"OB 64",
										"OB 65",
										"OB 66",
										"OB 67",
										"OB 68",
										"OB 69",
										"OB 70",
									],
									"Season 1": [
										"1.0",
										"1.1",
										"1.2",
										"1.3",
										"1.4",
										"1.5",
										"1.6",
										"1.7",
										"1.8",
										"1.9",
									],
									"Season 2": [
										"2.1",
										"2.2",
										"2.3",
										"2.4",
										"2.5",
										"2.6",
										"2.7",
										"2.8",
									],
									"Season 3": ["3.1", "3.2", "3.3", "3.4", "3.5"],
									"Season 4": ["4.1", "4.2", "4.3", "4.4", "4.5", "4.6", "4.7"],
									"Season 5": ["5.1", "5.2", "5.3", "5.4", "5.5", "5.6", "5.7"],
									"Season 6": [
										"6.1",
										"6.2",
										"6.3",
										"6.4",
										"6.5",
										"6.6",
										"6.6b",
									],
									"Season 7": ["7.1", "7.2", "7.3", "7.4", "7.5", "7.6", "7.7"],
									"Season 8": ["8.1"],
								}}
								placeholder="Select game version"
								fitViewport={false}
								direction="top"
								bind:value={selectedGameVersion}
							></Select>
						</div>
						{#if showAdvancedSettings}
							<div class="instance-setting flex flex-col gap-1">
								Installation path
								<Selector
									placeholder={chosenPath == ""
										? "Choose a directory"
										: chosenPath.length > 33
											? chosenPath.substring(0, 33) + "..."
											: chosenPath}
									bind:selected={chosenPath}
									type="folder"
									multiple={false}
								></Selector>
							</div>
						{/if}
					</div>
					<div class="add-instance">
						<div
							class="add-instace-buttons flex items-center justify-end pt-3 gap-2"
						>
							<Button
								onclick={toggleAdvancedSettings}
								icon={Code}
								class="text-text-color font-semibold"
								>{showAdvancedSettings
									? "Hide advanced"
									: "Show advanced"}
							</Button>
							<Button class="text-text-color font-semibold" icon={X}
								>Cancel</Button
							>
							<!-- TODO: SET chosenPath DEFAULT VALUE TO THE DEFAULT INSTALL LOCATION -->
							<Button
								class="text-text-color font-semibold"
								onclick={() =>
									addInstance({ label: instanceName, path: chosenPath })}
								icon={Plus}
								kind="accented"
								disabled={!canAddInstance()}>Add</Button
							>
						</div>
					</div>
				{:else}
					<div class="import-folder flex flex-col gap-2">
						<div class="zebi">
							<Button class="text-text-color font-semibold" icon={FolderPlus}>
								Import from folder</Button
							>
						</div>
						<div class="import-text flex gap-2 justify-start items-center">
							<Info class="text-xs"></Info>
							Or drag and drop your folder.
						</div>
					</div>
				{/if}
			</div>
		</div>
	</div>
{/if}

<main class="app-shell">
	{#if isTransitioning}
		<div
			class="transition-indicator"
			in:fade={{ duration: 200 }}
			out:fade={{ duration: 200 }}
		></div>
	{/if}

	<div class="sidebar">
		<div class="sidebar-section">
			{#each pageItems as item}
				{@const Icon = item.icon}
				<a class="sidebar-item" class:active={isActive(item)} href={item.path}>
					<Icon class="sidebar-item-icon" />
				</a>
			{/each}
			<div class="sidebar-section instance-list">
				{#if instances.current.length > 0}
					<div class="sidebar-seperator">
						<Minus preserveAspectRatio="none" />
					</div>

					{#each instances.current as instance, i}
						{#if i < 4}
							{@const path = `/library/${instance.id}`}
							<a
								class="sidebar-instance"
								class:active={isActive({ path })}
								href={path}
							>
								{"P"}
							</a>
						{/if}
					{/each}
				{/if}
			</div>
			<div class="sidebar-seperator">
				<Minus preserveAspectRatio="none" />
			</div>
			<button use:melt={$trigger} class="sidebar-item add-instance">
				<Plus />
			</button>
		</div>
		<div class="sidebar-section">
			<a
				class="sidebar-item"
				class:active={isActive({ path: "/settings" })}
				href="/settings"
			>
				<Settings class="sidebar-item-icon" />
			</a>
		</div>
	</div>
	<div class="content" class:transitioning={isTransitioning}>
		{#key page.url.pathname}
			<div
				class="page-content"
				in:fly={{
					x: 50,
					duration: 400,
					delay: 200,
					easing: cubicOut,
				}}
				out:fly={{
					x: -50,
					duration: 300,
					easing: cubicOut,
				}}
				onintrostart={() => (isTransitioning = true)}
				onintroend={() => (isTransitioning = false)}
				onoutrostart={() => (isTransitioning = true)}
				onoutroend={() => (isTransitioning = false)}
			>
				{@render children?.()}
			</div>
		{/key}
	</div>
</main>

<style>
	@reference "../../styles/global.css";

	.app-shell {
		@apply flex flex-row h-full overflow-hidden relative;
	}

	.transition-indicator {
		@apply absolute top-0 left-0 right-0 h-0.5 bg-gradient-to-r from-transparent via-background-200 to-transparent z-50;
		animation: shimmer 1s ease-in-out infinite;
	}

	@keyframes shimmer {
		0%,
		100% {
			opacity: 0.3;
		}
		50% {
			opacity: 0.8;
		}
	}

	.sidebar {
		@apply flex flex-col items-center justify-between border-r-2 bg-background-950 border-background-800;
	}

	.sidebar-section {
		@apply flex flex-col items-center gap-2 p-2;

		&.instance-list {
			@apply gap-4;
		}
	}

	.content {
		@apply flex-auto w-full h-screen overflow-hidden relative;
		transition: backdrop-filter 300ms ease-in-out;

		&.transitioning {
			backdrop-filter: blur(1px) brightness(0.98);
		}
	}

	.page-content {
		@apply w-full h-full overflow-auto;
		backface-visibility: hidden;
		transform: translateZ(0);
	}

	.sidebar-item {
		@apply grid place-items-center size-10 rounded-xl text-gray-400 transition-all duration-300 ease-in-out;
		transform: translateZ(0);

		&.active {
			@apply bg-secondary-800 text-primary-300;
			transform: scale(1.05);
			box-shadow: 0 2px 8px rgba(210, 223, 232, 0.1);
		}

		&:hover {
			@apply text-primary-300;
			transform: scale(1.02);
		}

		&:active {
			transform: scale(0.98);
		}

		& :global(svg) {
			@apply size-5;
			transition: transform 200ms ease-in-out;
		}

		&.active :global(svg) {
			transform: scale(1.1);
		}

		&.add-instance {
			cursor: pointer;
		}
	}

	.sidebar-seperator {
		@apply grid place-items-center w-8 h-2 text-background-500;

		& :global(svg) {
			height: 100%;
			width: 100%;
		}
	}

	.sidebar-instance {
		@apply grid place-items-center size-6 bg-purple-400 rounded-md text-background-100;

		&.active {
			@apply bg-blue-400;
		}
	}
</style>
