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

<input bind:value {type} {placeholder} autocomplete="off" autocorrect="off" {...props} />

<style>
	@reference "$lib/styles/global.css";

	input {
		@apply bg-background-800 focus-visible:ring-primary-500/60 h-10 w-full min-w-[220px] gap-2 rounded-lg px-3 text-sm ring-0 ring-transparent transition-colors focus-visible:ring-2 focus-visible:outline-none;
	}

	input:focus {
		@apply ring-primary-500 ring-2 outline-none;
	}
</style>
