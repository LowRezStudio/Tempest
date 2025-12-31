<script lang="ts">
	import "$lib/styles/global.css";
	import "@fontsource-variable/montserrat";
	import "@fontsource-variable/ubuntu-sans-mono";
	import Sidebar from "$lib/components/sidebar/Sidebar.svelte";
	import InstanceWizard from "$lib/components/library/InstanceWizard.svelte";
	import { instanceWizardOpen } from "$lib/stores/ui";
	import { onMount } from "svelte";
	import { page } from "$app/state";

	const { children } = $props();

	onMount(() => {
		document.addEventListener("contextmenu", function (e) {
			e.preventDefault();
		});

		document.addEventListener("dragstart", (event) => {
			event.preventDefault();
		});

		document.addEventListener("drop", (event) => {
			event.preventDefault();
		});
	});
</script>

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
