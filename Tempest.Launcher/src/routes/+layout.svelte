<script lang="ts">
	import "$lib/styles/global.css";
	// @ts-ignore
	import "@fontsource-variable/ubuntu-sans-mono";
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
	import { Tooltip } from "bits-ui";
	import favicon from "$lib/assets/favicon.ico?url";
	import AppShell from "$lib/components/layout/AppShell.svelte";
	import OnboardingPage from "$lib/components/onboarding/OnboardingPage.svelte";
	import { checkForCoreUpdatesAndInstall } from "$lib/core/mods";
	import { setQueryClient } from "$lib/queries/client";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { updaterStore } from "$lib/stores/updater.svelte";
	import type { Instance } from "$lib/types/instance";

	const { children } = $props();
	const queryClient = new QueryClient();
	setQueryClient(queryClient);

	let checkedLibrary = false;

	const inOnboarding = $derived(page.url.pathname === "/onboarding");

	$effect(() => {
		updaterStore.checkForUpdates(true);
	});

	let coreUpdateChecked = false;
	$effect(() => {
		if (coreUpdateChecked) return;
		const instances = Object.values(instanceMap.value).filter(Boolean) as Instance[];
		// Wait until instances are loaded (at least one) before checking - also runs if zero but we still want to record version
		if (
			!coreUpdateChecked &&
			instances.length === 0 &&
			Object.keys(instanceMap.value).length === 0
		)
			return;
		coreUpdateChecked = true;
		void checkForCoreUpdatesAndInstall(instances);
	});

	// Once per launch: route through onboarding when no game instances exist yet.
	$effect(() => {
		if (checkedLibrary) return;
		checkedLibrary = true;
		if (Object.keys(instanceMap.value).length === 0) void goto("/onboarding");
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<title>Tempest Launcher</title>
</svelte:head>

<QueryClientProvider client={queryClient}>
	<Tooltip.Provider delayDuration={400}>
		{#if inOnboarding}
			<!-- Full-screen first-run experience; finishing navigates back to "/". -->
			<OnboardingPage />
		{:else}
			<AppShell {children} />
		{/if}
	</Tooltip.Provider>
</QueryClientProvider>
