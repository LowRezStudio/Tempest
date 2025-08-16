<script lang="ts">
	import { goto } from "$app/navigation";
	import type { Icon } from "@lucide/svelte";
	import type { HTMLButtonAttributes } from "svelte/elements";

	interface Props extends HTMLButtonAttributes {
		icon?: typeof Icon;
		href?: string;
		kind?: "accented" | "danger" | "normal";
		size?: "square" | "normal";
	}

	let {
		children,
		href,
		icon,
		size,
		kind,
		...props
	}: Props = $props();
</script>

<button
	class:accented={kind == "accented"}
	class:danger={kind == "danger"}
	class:square={size == "square"}
	onclick={href ? () => goto(href) : undefined}
	{...props}
>
	<div class="wrapper flex justify-center items-center gap-3">
		{#if icon}
			{@const IconComponent = icon}
			<div class="icon">
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
		@apply text-[14px] font-semibold h-11 bg-background-800 px-3 rounded-xl flex items-center transition duration-150
			border-2 border-transparent;
	}

	button:hover {
		@apply cursor-pointer brightness-90;
	}

	button:disabled {
		@apply cursor-not-allowed brightness-75;
	}

	.accented {
		@apply border-primary-500 bg-secondary-800 text-primary-300;
	}

	.danger {
		@apply border-red-900 bg-red-700 text-white;
	}

	.square {
		@apply grid place-items-center size-11 px-0 py-0;
	}

	.square :global(svg) {
		@apply size-5;
	}
</style>
