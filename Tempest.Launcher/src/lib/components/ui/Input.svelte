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
		@apply h-10 rounded-lg bg-background-800 w-full min-w-[220px] px-3 text-sm transition-colors
			focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-500/60 gap-2 ring-0 ring-transparent;
	}

	input:focus {
		@apply ring-2 ring-primary-500 outline-none;
	}
</style>
