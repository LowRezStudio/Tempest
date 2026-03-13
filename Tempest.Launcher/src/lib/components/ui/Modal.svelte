<script lang="ts">
	import { X } from "@lucide/svelte";
	import { Dialog } from "bits-ui";
	import type { Snippet } from "svelte";

	interface Props {
		open?: boolean;
		title?: string;
		children: Snippet;
		actions?: Snippet;
		class?: string;
	}

	let {
		open = $bindable(false),
		title,
		children,
		actions,
		class: className = "",
	}: Props = $props();
</script>

<Dialog.Root bind:open>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/50" />
		<Dialog.Content
			class="fixed z-50 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-base-100 rounded-box shadow-xl max-w-2xl w-full max-h-[85vh] p-6 overflow-y-auto {className}"
			style="animation: pop-in 0.15s ease forwards;"
		>
			{#if title}
				<Dialog.Title class="font-bold text-lg pb-4">
					{title}
				</Dialog.Title>
			{/if}

			<Dialog.Close class="btn btn-circle btn-ghost absolute right-4 top-4">
				<X size={16} />
			</Dialog.Close>

			{@render children()}

			{#if actions}
				<div class="flex justify-end gap-2 pt-4">
					{@render actions()}
				</div>
			{/if}
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<style>
	@keyframes pop-in {
		from {
			opacity: 0;
			transform: translate(-50%, -50%) scale(0.95);
		}
		to {
			opacity: 1;
			transform: translate(-50%, -50%) scale(1);
		}
	}
</style>
