<script lang="ts">
	import { page } from "$app/state";
	import Button from "../ui/Button.svelte";
	import Input from "../ui/Input.svelte";
	import { addInstance, instances } from "$lib/state/instances.svelte";
	import { X } from "@lucide/svelte";
	import { createDialog, melt } from "@melt-ui/svelte";
	import {
		House,
		type Icon,
		Library,
		Minus,
		Plus,
		Settings,
		Swords,
		Users,
		Check,
		FolderCheck,
		CloudCheck,
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
	let canCreateInstance: boolean = $state(false);
	let selectedImportType: string = $state("download");

	const setImportType = (importType: string) => {
		selectedImportType = importType;
	};

	const toggleAdvancedSettings = () => {
		showAdvancedSettings = !showAdvancedSettings;
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
				max-w-[450px] -translate-x-1/2 -translate-y-1/2 rounded-xl bg-[#101013] shadow-lg
			"
			use:melt={$content}
		>
			<div class="modal-header p-5">
				<h2 use:melt={$title} class="m-0 text-lg font-semibold text-white">
					Adding an instance
				</h2>
			</div>
			<hr />
			<div class="modal-body p-5">
				<div class="instance-type flex flex-col gap-4 items-center pt-2 pb-5">
					<div
						class="instance-type-button flex items-center justify-center gap-4"
					>
						<Button
							onclick={() => setImportType("download")}
							kind={selectedImportType == "download" ? "accented" : "normal"}
							icon={selectedImportType == "download" ? CloudCheck : undefined}
							class="text-[#b0bac5] font-semibold">Download</Button
						>
						<Button
							onclick={() => setImportType("disk")}
							kind={selectedImportType == "disk" ? "accented" : "normal"}
							icon={selectedImportType == "disk" ? FolderCheck : undefined}
							>From Disk</Button
						>
					</div>
					<hr class="w-2xs" />
				</div>
				<div class="instance-settings">
					<div class="instance-setting flex flex-col gap-2">
						<label for="instance-name">Name</label>
						<Input id="instance-name"></Input>
					</div>
				</div>
			</div>

			<button
				use:melt={$close}
				aria-label="close"
				class="
					absolute right-4 top-4 inline-flex h-6 w-6 appearance-none
					items-center justify-center rounded-full p-1 text-blue-800
					hover:bg-blue-100 focus:shadow-blue-400
				"
			>
				<X class="size-4" />
			</button>
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
