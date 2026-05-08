<script lang="ts">
	import { page } from "$app/state";
	import type { Component } from "svelte";

	interface Props {
		href: string;
		icon: Component<any>;
		label: string;
		active?: boolean;
		circle?: boolean;
	}

	let { href, icon: Icon, label, active, circle }: Props = $props();

	let isActive = $derived(active ?? page.url.pathname === href);
</script>

<span class="wrapper" style="anchor-scope: --sidebar-item;">
	<a
		{href}
		class:rounded-full={circle}
		class={["btn btn-square", isActive ? "btn-accent" : "btn-ghost"]}
		style="anchor-name: --sidebar-item;"
		aria-label={label}
	>
		<Icon size={20} />
	</a>
	<div
		class="tip tooltip tooltip-right tooltip-open pointer-events-none fixed z-50 h-0 w-0"
		style="position-anchor: --sidebar-item; top: anchor(center); left: calc(anchor(right) + 4px); transform: translateY(-50%);"
		data-tip={label}
		aria-hidden="true"
	></div>
</span>

<style>
	.tip {
		opacity: 0;
		transition: opacity 100ms;
	}
	.wrapper:hover .tip,
	.wrapper:focus-within .tip {
		opacity: 1;
	}
</style>
