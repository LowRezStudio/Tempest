<script lang="ts">
	import { page } from "$app/state";
	import InstanceWizard from "$lib/components/library/InstanceWizard.svelte";
	import { instances } from "$lib/state/instances.svelte";
	import { House, type Icon, Library, Minus, Settings, Plus, Users } from "@lucide/svelte";
	import type { Snippet } from "svelte";
	import { cubicOut } from "svelte/easing";
	import { fade, fly } from "svelte/transition";

	let showInstanceWizard = $state(false);

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
		// { name: "Champions", path: "/champions", icon: Swords },
		{ name: "Servers", path: "/servers", icon: Users },
	];

	const isActive = (item: { path: string }) => page.url.pathname == item.path;
</script>

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
				<div class="sidebar-seperator">
					<Minus preserveAspectRatio="none" />
				</div>
			</div>
			<button onclick={() => (showInstanceWizard = true)} class="sidebar-item">
				<Plus class="size-5 transition-transform duration-200 ease-in-out" />
			</button>
			{#if showInstanceWizard}
				<InstanceWizard bind:open={showInstanceWizard} />
			{/if}
		</div>
		<div class="sidebar-section">
			<a class="sidebar-item" class:active={isActive({ path: "/settings" })} href="/settings">
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
	@reference "$lib/styles/global.css";

	.app-shell {
		@apply relative flex h-full flex-row overflow-hidden;
	}

	.transition-indicator {
		@apply via-background-200 absolute top-0 right-0 left-0 z-50 h-0.5 bg-gradient-to-r from-transparent to-transparent;
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
		@apply bg-background-950 border-background-800 flex flex-col items-center justify-between border-r-2;
	}

	.sidebar-section {
		@apply flex flex-col items-center gap-2 p-2;

		&.instance-list {
			@apply gap-4 p-0;
		}
	}

	.content {
		@apply relative h-screen w-full flex-auto overflow-hidden;
		transition: backdrop-filter 300ms ease-in-out;

		&.transitioning {
			backdrop-filter: blur(1px) brightness(0.98);
		}
	}

	.page-content {
		@apply h-full w-full overflow-auto;
		backface-visibility: hidden;
		transform: translateZ(0);
	}

	.sidebar-item {
		@apply grid size-10 cursor-pointer place-items-center rounded-xl text-gray-400 transition-all duration-300 ease-in-out;
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
	}

	.sidebar-seperator {
		@apply text-background-500 grid h-2 w-8 place-items-center;

		& :global(svg) {
			height: 100%;
			width: 100%;
		}
	}

	.sidebar-instance {
		@apply text-background-100 grid size-6 place-items-center rounded-md bg-purple-400;

		&.active {
			@apply bg-blue-400;
		}
	}
</style>
