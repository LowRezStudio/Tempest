<script lang="ts">
	import "$lib/styles/global.css";
	import "@fontsource-variable/montserrat";
	import "@fontsource-variable/ubuntu-sans-mono";
	import Sidebar from "$lib/components/sidebar/Sidebar.svelte";
	import InstanceWizard from "$lib/components/library/InstanceWizard.svelte";
	import { instanceWizardOpen } from "$lib/stores/ui";
	import { onMount } from "svelte";

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
	<main class="flex-1 overflow-y-auto min-w-0">
		{@render children?.()}
	</main>
	<InstanceWizard bind:open={$instanceWizardOpen} />
</div>
