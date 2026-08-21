<script lang="ts">
	import { page } from "$app/state";
	import { TriangleAlert } from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import Sidebar from "$lib/components/sidebar/Sidebar.svelte";
	import ErrorDetails from "$lib/components/ui/ErrorDetails.svelte";
	import LayoutDialogs from "$lib/components/ui/LayoutDialogs.svelte";
	import ToastStack from "$lib/components/ui/ToastStack.svelte";
	import { clearStaleConnectionIfNeeded } from "$lib/lobby/stores.svelte";
	import "$lib/platform/init.svelte";
	import "$lib/stores/flags.svelte";
	import {
		isDraggingFiles,
		showInstanceSelect,
		handleInstanceSelected,
		setOnModsInstalled,
	} from "$lib/mods/drop.svelte";
	import "$lib/platform/tray.svelte";
	import "$lib/theme/theme.svelte";
	import { m } from "$lib/paraglide/messages";
	import { localeState } from "$lib/stores/locale.svelte";

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

{#key localeState.current}
	<div class="flex h-screen w-full overflow-hidden">
		<Sidebar />
		<main class="relative min-w-0 flex-1 overflow-hidden">
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
		</main>
	</div>
{/key}
<!-- Dialogs live outside the locale key so an in-progress wizard keeps its
	state when the user commits a language change. -->
<LayoutDialogs
	isDraggingFiles={isDraggingFiles.value}
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
