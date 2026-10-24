<script lang="ts">
	import { RefreshCw, Search } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";

	interface Props {
		query: string;
		isLoading: boolean;
		onRefresh: () => void;
	}

	let { query = $bindable(), isLoading, onRefresh }: Props = $props();

	let searchInput = $state<HTMLInputElement | null>(null);

	function onWindowKey(e: KeyboardEvent) {
		if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f" && searchInput) {
			e.preventDefault();
			searchInput.focus();
			searchInput.select();
		}
	}

	function onSearchKey(e: KeyboardEvent) {
		if (e.key === "Escape" && query) {
			e.preventDefault();
			query = "";
		}
	}
</script>

<svelte:window onkeydown={onWindowKey} />

<label class="input input-sm w-56">
	<Search size={14} class="opacity-50" />
	<input
		type="search"
		class="grow"
		placeholder={m.mods_search_placeholder()}
		aria-label={m.common_search()}
		bind:this={searchInput}
		bind:value={query}
		onkeydown={onSearchKey}
	/>
</label>
<button
	class="btn btn-sm btn-square text-base-content/60"
	onclick={onRefresh}
	disabled={isLoading}
	aria-busy={isLoading}
	title={m.common_refresh()}
	aria-label={m.common_refresh()}
>
	{#if isLoading}
		<span class="loading loading-spinner size-3.5"></span>
	{:else}
		<RefreshCw size={14} />
	{/if}
</button>
