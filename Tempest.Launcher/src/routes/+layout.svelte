<script lang="ts">
	import "$lib/styles/global.css";
	import "@fontsource-variable/montserrat";
	import "@fontsource-variable/ubuntu-sans-mono";
	import Sidebar from "$lib/components/sidebar/Sidebar.svelte";

	const { children } = $props();
</script>

<div class="flex h-screen w-full overflow-hidden text-base-content">
	<Sidebar />
	<main class="flex-1 overflow-y-auto min-w-0">
		{@render children?.()}
	</main>
</div>
