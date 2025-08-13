<script lang="ts">
	import "@fontsource-variable/montserrat/wght.css";
	import "@fontsource-variable/montserrat/wght-italic.css";
	import "$lib/styles/global.css";
	import AppShell from "$lib/components/layout/AppShell.svelte";

	const { children } = $props();
</script>

<AppShell>
	{@render children()}
</AppShell>
