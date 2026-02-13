<script lang="ts">
	import type { Component } from "svelte";
	import { page } from "$app/state";
	import Tooltip from "$lib/components/ui/Tooltip.svelte";

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

<Tooltip label={label}>
	<a
		{href}
		class:rounded-full={circle}
		class={["btn btn-square", isActive ? "btn-accent" : "btn-ghost"]}
	>
		<Icon size={20} />
	</a>
</Tooltip>
