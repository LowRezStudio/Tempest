<script lang="ts">
	import { X } from "@lucide/svelte";
	import type { Snippet } from "svelte";

	interface Props {
		open: boolean;
		title: string;
		width?: string;
		children: Snippet;
		footer?: Snippet;
	}

	let { open = $bindable(), title, width = "max-w-2xl", children, footer }: Props = $props();
</script>

{#if open}
	<div class="modal modal-open" role="dialog" aria-modal="true" aria-label={title}>
		<div class="modal-box flex max-h-[85vh] flex-col p-0 {width}">
			<div class="border-base-300 flex items-center justify-between border-b px-4 py-2">
				<h3 class="text-base font-semibold">{title}</h3>
				<button
					class="btn btn-circle btn-ghost btn-sm"
					onclick={() => (open = false)}
					aria-label="Close"
				>
					<X size={16} />
				</button>
			</div>
			<div class="min-h-0 flex-1 overflow-auto px-4 py-3">
				{@render children()}
			</div>
			{#if footer}
				<div class="border-base-300 flex justify-end gap-2 border-t px-4 py-2">
					{@render footer()}
				</div>
			{/if}
		</div>
		<button class="modal-backdrop" onclick={() => (open = false)} aria-label="Close"></button>
	</div>
{/if}
