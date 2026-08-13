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
		onsubmit?: (e: SubmitEvent) => void;
	}

	let {
		open = $bindable(false),
		title,
		children,
		actions,
		class: className = "",
		onsubmit,
	}: Props = $props();
</script>

<Dialog.Root bind:open>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/50" />
		<Dialog.Content
			class="bg-base-100 rounded-box fixed top-1/2 left-1/2 z-50 max-h-[85vh] w-full max-w-2xl -translate-x-1/2 -translate-y-1/2 overflow-y-auto p-6 shadow-xl {className}"
			style="animation: pop-in 0.15s ease forwards;"
		>
			<form
				onsubmit={(e) => {
					e.preventDefault();
					if (onsubmit) {
						onsubmit(e);
					}
				}}
				class="contents"
			>
				{#if title}
					<Dialog.Title class="pb-4 text-lg font-bold">
						{title}
					</Dialog.Title>
				{/if}

				<Dialog.Close class="btn btn-circle btn-ghost absolute top-4 right-4" type="button">
					<X size={16} />
				</Dialog.Close>

				{@render children()}

				{#if actions}
					<div class="flex justify-end gap-2 pt-4">
						{@render actions()}
					</div>
				{/if}
			</form>
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
