<script lang="ts">
	import "$lib/styles/global.css";
	// @ts-ignore
	import "@fontsource-variable/ubuntu-sans-mono";
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
	import { Tooltip } from "bits-ui";
	import { untrack } from "svelte";
	import favicon from "$lib/assets/favicon.ico?url";
	import AppShell from "$lib/components/layout/AppShell.svelte";
	import SigningKeyModal from "$lib/components/mods/SigningKeyModal.svelte";
	import OnboardingPage from "$lib/components/onboarding/OnboardingPage.svelte";
	import { restoreRunningGames } from "$lib/core/launch";
	import { checkForCoreUpdatesAndInstall } from "$lib/core/mods";
	import { startInstallQueue } from "$lib/mods/installQueue.svelte";
	import { checkModUpdates } from "$lib/mods/updates.svelte";
	import { invoke } from "$lib/platform/api";
	import { setQueryClient } from "$lib/queries/client";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { snapshotPaused } from "$lib/stores/snapshot.svelte";
	import { updaterStore } from "$lib/stores/updater.svelte";
	import type { Instance } from "$lib/types/instance";

	const { children } = $props();
	const queryClient = new QueryClient();
	setQueryClient(queryClient);

	// Periodically dump localStorage to settings.json in the profile dir as a
	// recoverable snapshot (used by the migration path when switching storage
	// backends). The file itself is written host-side.
	$effect(() => {
		let lastSnapshot = "";
		const snapshot = async () => {
			if (snapshotPaused.value) return;
			try {
				const dump: Record<string, string> = {};
				for (let i = 0; i < localStorage.length; i++) {
					const k = localStorage.key(i);
					if (k) dump[k] = localStorage.getItem(k) ?? "";
				}
				const text = JSON.stringify(dump, null, 2);
				if (text === lastSnapshot) return;
				lastSnapshot = text;
				await invoke("settings:write-snapshot", { json: text });
			} catch (error) {
				console.warn("[settings] snapshot skipped:", error);
			}
		};
		void snapshot();
		const timer = setInterval(snapshot, 30_000);
		return () => clearInterval(timer);
	});

	let checkedLibrary = false;

	// Vite full reloads (and renderer crashes) drop the tracked games and the
	// restore queue while the host keeps their children running: pick them back
	// up from the host instead of starting over.
	let restored = false;
	let gamesRestored: Promise<unknown> = Promise.resolve();
	$effect(() => {
		if (restored) return;
		restored = true;
		// Queued mod installs wait for their game to close, so they start once the running
		// games are known.
		gamesRestored = restoreRunningGames().finally(startInstallQueue);
		void restoreQueue.adoptRunning();
	});

	const inOnboarding = $derived(page.url.pathname === "/onboarding");

	$effect(() => {
		untrack(() => updaterStore.checkForUpdates(true));
	});

	// Mod updates need to know which games run, so they wait for those to be restored.
	let modUpdatesReady = false;
	const checkMods = () => {
		if (modUpdatesReady && !inOnboarding) void checkModUpdates();
	};

	let coreUpdateChecked = false;
	$effect(() => {
		if (coreUpdateChecked) return;
		const instances = Object.values(instanceMap.value).filter(Boolean) as Instance[];
		if (instances.length === 0 && Object.keys(instanceMap.value).length === 0) return;
		coreUpdateChecked = true;
		void Promise.allSettled([gamesRestored, checkForCoreUpdatesAndInstall(instances)]).then(
			() => {
				modUpdatesReady = true;
				checkMods();
			},
		);
	});

	// People leave updates waiting, so every return to the launcher looks again and asks again.
	let away = false;
	function returned() {
		if (!away) return;
		away = false;
		void updaterStore.checkForUpdates(true);
		checkMods();
	}

	// Once per launch: route through onboarding when no game instances exist yet.
	$effect(() => {
		if (checkedLibrary) return;
		checkedLibrary = true;
		if (Object.keys(instanceMap.value).length === 0) void goto("/onboarding");
	});
</script>

<svelte:window onblur={() => (away = true)} onfocus={returned} />

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

<!-- Global singleton: any surface can ask for a signing key name. -->
<SigningKeyModal />
