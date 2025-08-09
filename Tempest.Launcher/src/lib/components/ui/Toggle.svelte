<script lang="ts">
	import { createSwitch, melt } from "@melt-ui/svelte";

	interface Props {
		label?: string;
		value?: boolean;
		onValueChange?: (value: boolean | undefined) => void;
	}

	let { label, value = $bindable(), onValueChange }: Props = $props();

	const {
		elements: { root, input },
		states: { checked },
	} = createSwitch({
		defaultChecked: value,
		onCheckedChange: ({ next }) => {
			value = next;
			onValueChange?.(next);
			return next;
		},
	});
</script>

<form>
	<div class="flex items-center justify-evenly">
		<label
			class="pr-4 leading-none text-text-color"
			for="airplane-mode"
			id="airplane-mode-label"
		>
			{label}
		</label>
		<button
			use:melt={$root}
			class="relative h-6 cursor-pointer rounded-full bg-background-600 transition-colors data-[state=checked]:bg-primary-500"
			id="airplane-mode"
			aria-labelledby="airplane-mode-label"
		>
			<span class="thumb block rounded-full bg-white transition" />
		</button>
		<input use:melt={$input} />
	</div>
</form>

<style>
	button {
		--w: 2.75rem;
		--padding: 0.125rem;
		width: var(--w);
	}

	.thumb {
		--size: 1.25rem;
		width: var(--size);
		height: var(--size);
		transform: translateX(var(--padding));
	}

	:global([data-state="checked"]) .thumb {
		transform: translateX(calc(var(--w) - var(--size) - var(--padding)));
	}
</style>
