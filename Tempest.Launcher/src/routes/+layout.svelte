<script lang="ts">
	import "$lib/styles/global.css";
	import "@fontsource-variable/montserrat";
	import "@fontsource-variable/ubuntu-sans-mono";
	import Sidebar from "$lib/components/sidebar/Sidebar.svelte";
	import InstanceWizard from "$lib/components/library/InstanceWizard.svelte";
	import ToastStack from "$lib/components/ui/ToastStack.svelte";
	import { instanceWizardOpen } from "$lib/stores/ui";
	import { page } from "$app/state";
	import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
	import { platform } from "@tauri-apps/plugin-os";
	import { polyfillCountryFlagEmojis } from "country-flag-emoji-polyfill";

	const { children } = $props();
	const queryClient = new QueryClient();

	$effect(() => {
		polyfillCountryFlagEmojis();
		document.documentElement.dataset.platform = platform();

		const handleContextMenu = (event: Event) => {
			event.preventDefault();
		};
		const handleDragStart = (event: Event) => {
			event.preventDefault();
		};
		const handleDrop = (event: Event) => {
			event.preventDefault();
		};

		document.addEventListener("contextmenu", handleContextMenu);
		document.addEventListener("dragstart", handleDragStart);
		document.addEventListener("drop", handleDrop);

		return () => {
			document.removeEventListener("contextmenu", handleContextMenu);
			document.removeEventListener("dragstart", handleDragStart);
			document.removeEventListener("drop", handleDrop);
		};
	});
</script>

<QueryClientProvider client={queryClient}>
	<div class="flex h-screen w-full overflow-hidden">
		<Sidebar />
		<main class="flex-1 min-w-0 relative overflow-hidden">
			{#key page.url.pathname}
				<div class="page-transition">
					{@render children?.()}
				</div>
			{/key}
		</main>
		<InstanceWizard bind:open={$instanceWizardOpen} />
	</div>
	<ToastStack />
</QueryClientProvider>

<style>
	.page-transition {
		position: absolute;
		inset: 0;
		overflow-y: auto;
		animation: page-enter 250ms var(--ease-snappy) both;
	}

	:global(.page-transition:has(~ .page-transition)) {
		animation: page-exit 250ms var(--ease-smooth) both;
	}
</style>
