<script lang="ts">
	import { page } from "$app/state";
	import { useQueryClient } from "@tanstack/svelte-query";
	import Sidebar from "$lib/components/sidebar/Sidebar.svelte";
	import LayoutDialogs from "$lib/components/ui/LayoutDialogs.svelte";
	import ToastStack from "$lib/components/ui/ToastStack.svelte";
	import "$lib/platform/init.svelte";
	import { isDraggingFiles, showInstanceSelect, handleInstanceSelected, setOnModsInstalled } from "$lib/mods/drop.svelte";
	import "$lib/lobby/close-guard.svelte";
	import "$lib/theme/theme.svelte";
	import { clearStaleConnectionIfNeeded } from "$lib/lobby/stores.svelte";
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
		<main class="flex-1 min-w-0 relative overflow-hidden">
			{#key page.url.pathname}
				<div class="page-transition">
					{@render children?.()}
				</div>
			{/key}
		</main>
		<LayoutDialogs
			isDraggingFiles={isDraggingFiles.value}
			bind:showInstanceSelect={localShowInstanceSelect}
			onselect={handleInstanceSelected}
			oncancel={() => {}}
		/>
	</div>
	<ToastStack />
{/key}

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
