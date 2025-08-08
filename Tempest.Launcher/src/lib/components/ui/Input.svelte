<script lang="ts">
	import { goto } from "$app/navigation";
	import type { Icon } from "@lucide/svelte";
	import type { HTMLInputAttributes } from "svelte/elements";

	interface Props extends HTMLInputAttributes {
		type?: "text" | "password";
		kind?: "normal" | "icon";
		icon?: typeof Icon;
		placeholder?: string;
	}

	let {
		children,
		type = "text",
		kind = "normal",
		placeholder,
		icon,
		...props
	}: Props = $props();
</script>

<input
type={type}
placeholder={placeholder}
autocomplete="off"
autocorrect="off"
{... props}
>

<style>
	@reference "../../styles/global.css";
    
	input {
		@apply bg-[var(--component-background)] rounded-xl;
	}
</style>
