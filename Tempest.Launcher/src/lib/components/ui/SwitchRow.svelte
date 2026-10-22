<script lang="ts">
	import type { Settings } from "@lucide/svelte";

	interface Props {
		checked: boolean;
		title: string;
		hint?: string;
		icon?: typeof Settings;
		disabled?: boolean;
	}

	let { checked = $bindable(), title, hint, icon: Icon, disabled = false }: Props = $props();
</script>

<!-- An on/off setting as a whole clickable row, after the mod website's listed/unlisted switch -->
<label
	class={[
		"bg-base-100 border-base-300 rounded-field flex items-center gap-3 border p-3 transition-colors",
		disabled ? "opacity-60" : "hover:border-primary/50 cursor-pointer",
	]}
>
	{#if Icon}
		<span
			class={[
				"flex size-8 shrink-0 items-center justify-center rounded-full transition-colors",
				checked ? "bg-primary/15 text-primary" : "bg-base-300 text-base-content/60",
			]}><Icon size={16} /></span
		>
	{/if}
	<span class="min-w-0 flex-1">
		<span class="block text-sm font-medium">{title}</span>
		{#if hint}<span class="text-base-content/60 block text-xs">{hint}</span>{/if}
	</span>
	<input type="checkbox" class="toggle toggle-primary shrink-0" bind:checked {disabled} />
</label>
