<script lang="ts">
	import { X } from "@lucide/svelte";
	import { Dialog, type WithoutChild } from "bits-ui";
	import type { Snippet } from "svelte";

	type Props = Dialog.RootProps & {
		trigger?: Snippet;
		title: string;
		contentProps?: WithoutChild<Dialog.ContentProps>;
	};

	let {
		open = $bindable(false),
		children,
		contentProps,
		title,
		trigger,
		...restProps
	}: Props = $props();
</script>

<Dialog.Root bind:open {...restProps}>
	{#if trigger}
		{@render trigger()}
	{/if}
	<Dialog.Portal>
		<Dialog.Overlay>
			<div class="fixed inset-0 z-50 bg-black/50"></div>
		</Dialog.Overlay>
		<Dialog.Content {...contentProps}>
			<div
				class="fixed left-1/2 top-1/2 z-50 max-h-[85vh] w-[90vw] max-w-[550px] -translate-x-1/2 -translate-y-1/2 rounded-xl bg-background-950 shadow-lg"
			>
				<Dialog.Title class="p-5 flex justify-between items-center">
					<h2 class="m-0 text-lg font-semibold text-white">
						{title}
					</h2>

					<Dialog.Close
						aria-label="close"
						class="rounded-full p-2 cursor-pointer text-text-color bg-component-background transition duration-150 hover:brightness-90 focus:shadow-white"
					>
						<X class="size-6" />
					</Dialog.Close>
				</Dialog.Title>
				<hr class="border-background-700 mx-5" />
				<div class="modal-body p-5">
					{@render children?.()}
				</div>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
