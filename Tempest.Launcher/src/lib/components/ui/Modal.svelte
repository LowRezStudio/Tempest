<script>
  import { createDialog, melt } from '@melt-ui/svelte';
  import { fade, fly } from 'svelte/transition';
  import { X } from "@lucide/svelte";
  import { untrack } from 'svelte';

  let { showModal = $bindable(), children } = $props();

	const {
		elements: {
			trigger,
			overlay,
			content,
			title,
			description,
			close,
			portalled,
		},
		states: { open },
	} = createDialog({
		forceVisible: true,
	});

  </script>
  
  {#if $open || showModal}
	<div class="" use:melt={$portalled} onclose={showModal = false}>
		<div
			use:melt={$overlay}
			class="fixed inset-0 z-50 bg-black/50"
			transition:fade={{ duration: 150 }}
		></div>
		<div
			class="
				fixed left-1/2 top-1/2 z-50 max-h-[85vh] w-[90vw]
				max-w-[550px] -translate-x-1/2 -translate-y-1/2 rounded-xl bg-[#101013] shadow-lg
			"
			use:melt={$content}
		>
			<div class="modal-header p-5 flex justify-between items-center">
				<h2 use:melt={$title} class="m-0 text-lg font-semibold text-white">
					Adding an instance
				</h2>

				<button
					use:melt={$close}
					aria-label="close"
					class=" rounded-full p-2 cursor-pointer text-text-color bg-component-background transition duration-150 hover:brightness-90 focus:shadow-white"
				>
					<X class="size-6" />
				</button>
			</div>
			<hr class="border-[#222329]" />
			<div class="modal-body p-5">
        {@render children?.()}
			</div>
		</div>
	</div>
{/if}