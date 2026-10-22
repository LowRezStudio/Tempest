<script lang="ts">
	import { CircleAlert } from "@lucide/svelte";

	interface Props {
		/** Referenced by the field's aria-describedby */
		id?: string;
		title: string;
		message?: string;
		/** A one-click correction, e.g. the value the user probably meant */
		fix?: { label: string; apply: () => void };
		class?: string;
	}

	let { id, title, message, fix, class: className = "" }: Props = $props();
</script>

<!-- What is wrong with a field's value; pairs with aria-invalid on the field -->
<div
	{id}
	class={[
		"border-error/50 bg-error/10 rounded-field flex items-start gap-2 border px-3 py-2 text-xs wrap-anywhere",
		className,
	]}
>
	<CircleAlert size={16} class="text-error mt-px shrink-0" />
	<div class="min-w-0 flex-1">
		<p class="text-error font-semibold">{title}</p>
		{#if message}<p class="text-base-content/80 mt-0.5">{message}</p>{/if}
	</div>
	{#if fix}
		<button type="button" class="btn btn-error btn-xs shrink-0" onclick={fix.apply}>
			{fix.label}
		</button>
	{/if}
</div>
