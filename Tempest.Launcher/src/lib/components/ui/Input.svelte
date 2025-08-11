<script lang="ts">
	import type { Icon } from "@lucide/svelte";
	import type { HTMLInputAttributes } from "svelte/elements";

	interface Props extends HTMLInputAttributes {
		type?: "text" | "password";
		kind?: "normal" | "icon";
		icon?: typeof Icon;
		placeholder?: string;
		value?: string;
	}

	let {
		children,
		type = "text",
		kind = "normal",
		placeholder,
		icon,
		value = $bindable(),
		...props
	}: Props = $props();
</script>

<input bind:value={value} {type} {placeholder} autocomplete="off" autocorrect="off" {...props} />

<style>
	@reference "../../styles/global.css";

	input {
		@apply bg-[var(--component-background)] rounded-xl h-[2.3rem] p-4 text-[var(--text-color)] font-[550] transition
			duration-150;
	}
	input:focus {
		@apply ring-4 ring-[var(--color-primary)] outline-none;
	}
</style>
