<script lang="ts">
	import type { Instance } from "$lib/types/instance";
	import { Box } from "@lucide/svelte";

	interface Props {
		instance: Instance;
	}

	let { instance }: Props = $props();

	let isSettingUp = $derived((instance.state as { type: string }).type === "setup");
</script>

<a
	class="bg-base-200 hover:bg-base-300 rounded-lg transition-all duration-200 cursor-pointer p-4 text-left"
	href={`/instance/${instance.id}`}
>
	<div class="flex items-center gap-3">
		<div
			class="w-12 h-12 rounded-lg bg-base-100 flex items-center justify-center shrink-0 overflow-hidden"
		>
			{#if isSettingUp}
				<div class="tooltip" data-tip="Setting up instance...">
					<span class="loading loading-spinner loading-sm"></span>
				</div>
			{:else}
				<Box size={24} class="opacity-60" />
			{/if}
		</div>
		<div class="flex-1 min-w-0">
			<h3 class="font-bold text-base truncate mb-0.5">{instance.label}</h3>
			{#if instance.version}
				<p class="text-sm opacity-70 font-mono flex items-center gap-1.5">
					<Box size={12} />
					<span>{instance.version}</span>
				</p>
			{/if}
		</div>
	</div>
</a>
