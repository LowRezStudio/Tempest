<script lang="ts">
	import { removeToast, toasts } from "$lib/stores/ui";
	import type { Toast } from "$lib/stores/ui";

	interface Props {
		placement?: string;
	}

	let { placement = "toast-top toast-end" }: Props = $props();

	let toastItems = $derived($toasts);

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
		<div class={getAlertClass(toast)} role="status">
			<div class="flex items-start gap-3">
				<div class="flex-1">
					{#if toast.title}
						<div class="font-semibold text-sm">{toast.title}</div>
					{/if}
					<div class="text-sm">{toast.message}</div>
				</div>
				{#if toast.dismissible !== false}
					<button class="btn btn-ghost btn-xs" onclick={() => removeToast(toast.id)}>
						Dismiss
					</button>
				{/if}
			</div>
		</div>
	{/each}
</div>
