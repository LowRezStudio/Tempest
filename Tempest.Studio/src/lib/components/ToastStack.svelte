<script lang="ts">
	import { X } from "@lucide/svelte";
	import { toasts } from "$lib/studio/toast.svelte";
</script>

<div class="toast toast-end toast-bottom z-50 w-[min(28rem,calc(100vw-2rem))]" aria-live="polite">
	{#each toasts.items as toast (toast.id)}
		<div
			class="alert alert-{toast.tone} relative block whitespace-normal shadow-lg"
			role="status"
		>
			<!-- text-current: the button's own colour is the page's, which vanishes on a tinted toast -->
			<button
				class="btn btn-circle btn-ghost btn-sm absolute top-1 right-1 text-current opacity-70 hover:opacity-100"
				onclick={() => toasts.remove(toast.id)}
				title="Dismiss"
				aria-label="Dismiss"
			>
				<X size={16} />
			</button>
			<div class="pr-8">
				{#if toast.title}
					<div class="text-sm font-semibold">{toast.title}</div>
				{/if}
				<div class="selectable text-sm [overflow-wrap:anywhere] break-words">
					{toast.message}
				</div>
			</div>
		</div>
	{/each}
</div>
