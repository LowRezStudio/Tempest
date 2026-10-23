<script lang="ts">
	import { X } from "@lucide/svelte";
	import { toasts } from "$lib/studio/toast.svelte";
</script>

<div class="toast toast-end toast-bottom z-50 max-w-md" aria-live="polite">
	{#each toasts.items as toast (toast.id)}
		<div
			class="alert alert-{toast.tone} relative block whitespace-normal shadow-lg"
			role="status"
		>
			<button
				class="btn btn-circle btn-ghost btn-xs absolute top-1 right-1"
				onclick={() => toasts.remove(toast.id)}
				aria-label="Dismiss"
			>
				<X size={14} />
			</button>
			<div class="pr-6">
				{#if toast.title}
					<div class="text-sm font-semibold">{toast.title}</div>
				{/if}
				<div class="selectable text-sm break-words">{toast.message}</div>
			</div>
		</div>
	{/each}
</div>
