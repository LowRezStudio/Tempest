<script lang="ts">
	import { page } from "$app/state";
	import { TriangleAlert } from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { fade } from "svelte/transition";
	import Sidebar from "$lib/components/sidebar/Sidebar.svelte";
	import ErrorDetails from "$lib/components/ui/ErrorDetails.svelte";
	import LayoutDialogs from "$lib/components/ui/LayoutDialogs.svelte";
	import ToastStack from "$lib/components/ui/ToastStack.svelte";
	import "$lib/platform/init.svelte";
	import "$lib/stores/flags.svelte";
	import { clearStaleConnectionIfNeeded } from "$lib/lobby/stores.svelte";
	import "$lib/mods/deeplink.svelte";
	import "$lib/platform/tray.svelte";
	import "$lib/theme/theme.svelte";
	import {
		showInstanceSelect,
		handleInstanceSelected,
		setOnModsInstalled,
	} from "$lib/mods/drop.svelte";
	import { m } from "$lib/paraglide/messages";
	import { localeState } from "$lib/stores/locale.svelte";
	import { currentWeather, onKonamiKey } from "$lib/theme/weather.svelte";
	import SeasonalDecorations from "./SeasonalDecorations.svelte";
	import SeasonalWeather from "./SeasonalWeather.svelte";

	clearStaleConnectionIfNeeded();

	const { children } = $props();
	const queryClient = useQueryClient();
	setOnModsInstalled((path) => queryClient.invalidateQueries({ queryKey: ["mods", path] }));

	let localShowInstanceSelect = $state(false);
	$effect(() => {
		localShowInstanceSelect = showInstanceSelect.value;
	});
	$effect(() => {
		showInstanceSelect.value = localShowInstanceSelect;
	});
</script>

<svelte:window onkeydown={onKonamiKey} />

{#key localeState.current}
	<div class="flex h-screen w-full overflow-hidden">
		<Sidebar />
		<!-- Pages leave their background to main, so the weather falls behind their cards -->
		<main class="bg-base-100 relative min-w-0 flex-1 overflow-hidden">
			{#if currentWeather.value?.kind}
				{@const kind = currentWeather.value.kind}
				{#key kind}
					<div class="absolute inset-0" transition:fade={{ duration: 600 }}>
						<SeasonalWeather {kind} />
					</div>
				{/key}
			{/if}
			<svelte:boundary>
				{#key page.url.pathname}
					<div class="page-transition">
						{@render children?.()}
					</div>
				{/key}

				{#snippet failed(error)}
					<div
						class="flex h-full flex-col items-center justify-center gap-4 p-6 text-center"
					>
						<div role="alert" class="alert alert-error max-w-md">
							<TriangleAlert />
							<div>
								<p class="font-semibold">{m.error_section_title()}</p>
								<p class="text-sm opacity-80">{m.error_section_description()}</p>
							</div>
							<button
								class="btn btn-outline btn-sm"
								onclick={() => location.reload()}
							>
								{m.error_reload()}
							</button>
						</div>
						<ErrorDetails {error} class="w-full max-w-md" />
					</div>
				{/snippet}
			</svelte:boundary>
			<SeasonalDecorations />
		</main>
	</div>
{/key}
<!-- Dialogs live outside the locale key so an in-progress wizard keeps its
	state when the user commits a language change. -->
<LayoutDialogs
	bind:showInstanceSelect={localShowInstanceSelect}
	onselect={handleInstanceSelected}
	oncancel={() => {}}
/>
<ToastStack />

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
