<script lang="ts">
	import { X } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { removeToast, toasts } from "$lib/stores/ui.svelte";
	import type { Toast } from "$lib/stores/ui.svelte";

	interface Props {
		placement?: string;
	}

	let { placement = "toast-bottom toast-end" }: Props = $props();

	let toastItems = $derived(toasts.value);

	const getAlertClass = (toast: Toast) => {
		const tone = toast.tone ?? "info";
		return tone === "neutral" ? "alert" : `alert alert-${tone}`;
	};
</script>

<div
	class={["toast", placement, "z-50"].filter(Boolean).join(" ")}
	aria-live="polite"
	aria-atomic="true"
>
	{#each toastItems as toast (toast.id)}
		<div class="{getAlertClass(toast)} relative" role="status">
			{#if toast.dismissible !== false}
				<button
					class="btn btn-circle btn-ghost btn-xs absolute top-2 right-2"
					onclick={() => removeToast(toast.id)}
					aria-label={m.common_dismiss()}
				>
					<X size={14} />
				</button>
			{/if}
			<div class="pr-6">
				{#if toast.title}
					<div class="font-semibold text-sm">{toast.title}</div>
				{/if}
				<div class="text-sm">{toast.message}</div>
			</div>
		</div>
	{/each}
</div>
