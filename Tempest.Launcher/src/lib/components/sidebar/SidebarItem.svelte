<script lang="ts">
	import type { Component } from "svelte";
	import { page } from "$app/state";

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

<div class="tooltip tooltip-right" data-tip={label}>
	<a
		{href}
		class:rounded-full={circle}
		class={["btn btn-square", isActive ? "btn-accent" : "btn-ghost"]}
	>
		<Icon size={20} />
	</a>
</div>
