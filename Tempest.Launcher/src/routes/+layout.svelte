<script lang="ts">
	import "$lib/styles/global.css";
	// @ts-ignore
	import "@fontsource-variable/ubuntu-sans-mono";
	import { Tooltip } from "bits-ui";
	import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
	import favicon from "$lib/assets/favicon.ico?url";
	import AppShell from "$lib/components/layout/AppShell.svelte";
	import { updaterStore } from "$lib/stores/updater.svelte";

	const { children } = $props();
	const queryClient = new QueryClient();

	$effect(() => {
		updaterStore.checkForUpdates(true);
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon}>
</svelte:head>

<QueryClientProvider client={queryClient}>
	<Tooltip.Provider delayDuration={400}>
		<AppShell {children} />
	</Tooltip.Provider>
</QueryClientProvider>
