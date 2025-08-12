<script lang="ts">
	import { goto } from "$app/navigation";
	import type { Icon } from "@lucide/svelte";
	import type { HTMLButtonAttributes } from "svelte/elements";

	interface Props extends HTMLButtonAttributes {
		icon?: typeof Icon;
		href?: string;
		kind?: "accented" | "normal";
	}

	let {
		children,
		href,
		icon,
		kind,
		...props
	}: Props = $props();
</script>

<button
	class:accented={kind == "accented"}
	onclick={href ? () => goto(href) : undefined}
	{...props}
>
	<div class="wrapper flex justify-center items-center gap-3">
		{#if icon}
			{@const IconComponent = icon}
			<div class="icon text-white">
				<IconComponent />
			</div>
		{/if}
		{#if children}
			<span>
				{@render children()}
			</span>
		{/if}
	</div>
</button>

<style>
	@reference "../../styles/global.css";

	button {
		@apply text-[14px] h-10 bg-background-800 px-3 rounded-xl flex items-center border-2 border-transparent transition
			duration-150;
	}

	button:hover {
		@apply cursor-pointer border-transparent brightness-90;
	}

	button:disabled {
		@apply cursor-not-allowed brightness-75;
	}

	.accented {
		@apply ring-2 ring-primary-500 bg-secondary-800 text-primary-300 shadow-lg;
	}

	.accented:hover {
		@apply ring-2 ring-primary-500 shadow-lg;
	}
</style>
