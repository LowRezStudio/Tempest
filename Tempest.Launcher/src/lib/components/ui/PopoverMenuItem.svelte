<script lang="ts">
	import type { Snippet } from "svelte";
	import type { HTMLButtonAttributes } from "svelte/elements";

	interface Props extends HTMLButtonAttributes {
		children: Snippet;
		class?: string;
	}

	let { children, class: className = "", ...restProps }: Props = $props();
</script>

<li role="menuitem">
	<button class="cursor-pointer {className}" {...restProps}>
		{@render children()}
	</button>
</li>
