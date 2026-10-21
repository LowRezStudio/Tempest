<script lang="ts">
	import { X } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { removeToast, toasts } from "$lib/stores/ui.svelte";
	import type { Toast, ToastTone } from "$lib/stores/ui.svelte";

	interface Props {
		placement?: string;
	}

	let { placement = "toast-bottom toast-end" }: Props = $props();

	let toastItems = $derived(toasts.value);

	// Spelled out: Tailwind only generates classes it finds literally in the source.
	const alertClasses: Record<ToastTone, string> = {
		info: "alert alert-info",
		success: "alert alert-success",
		warning: "alert alert-warning",
		error: "alert alert-error",
		neutral: "alert",
	};

	const getAlertClass = (toast: Toast) => alertClasses[toast.tone ?? "info"];
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
					<div class="text-sm font-semibold">{toast.title}</div>
				{/if}
				<div class="text-sm">{toast.message}</div>
			</div>
		</div>
	{/each}
</div>
