<script lang="ts">
	import type { Instance } from "$lib/types/instance";
	import { Box, Loader2 } from "@lucide/svelte";

	interface Props {
		instance: Instance;
	}

	let { instance }: Props = $props();

	let isBusy = $derived(instance.state.type !== "prepared");
	let statusLabel = $derived.by(() => {
		if (instance.state.type !== "unprepared") return "";
		if (instance.state.status === "downloading") return "Downloading build data...";
		if (instance.state.status === "paused") return "Download paused";
		return "Dumping tokens...";
	});
	let containerClasses = $derived([
		"bg-base-200 rounded-lg transition-all duration-200 p-4 text-left",
		isBusy ? "cursor-not-allowed opacity-70" : "hover:bg-base-300 cursor-pointer",
	]);
</script>

{#snippet content()}
	<div class="flex items-center gap-3">
		<div
			class="w-12 h-12 rounded-lg bg-base-100 flex items-center justify-center shrink-0 overflow-hidden"
		>
			<Box size={24} class="opacity-60" />
		</div>
		<div class="flex-1 min-w-0">
			<h3 class="font-bold text-base truncate mb-0.5">{instance.label}</h3>
			{#if instance.version}
				<p class="text-sm opacity-70 font-mono flex items-center gap-1.5">
					<Box size={12} />
					<span>{instance.version}</span>
				</p>
			{/if}
			{#if isBusy}
				<p class="text-xs opacity-70 flex items-center gap-1.5">
					<Loader2 size={12} class="animate-spin" />
					<span>{statusLabel}</span>
				</p>
			{/if}
		</div>
	</div>
{/snippet}

{#if isBusy}
	<div class={containerClasses} aria-disabled="true" aria-busy="true">
		{@render content()}
	</div>
{:else}
	<a class={containerClasses} href={`/instance/${instance.id}`}>
		{@render content()}
	</a>
{/if}
