<!--
	A dropdown menu built from command ids (shortcuts and enabled state come
	from the command registry); "-" is a separator, a `{ label, run }` object
	an ad-hoc item (recent folders…).
-->
<script module lang="ts">
	export type { MenuItem } from "./MenuItems.svelte";
</script>

<script lang="ts">
	import MenuItems, { type MenuItem } from "./MenuItems.svelte";
	import type { Snippet } from "svelte";

	interface Props {
		items: MenuItem[];
		/** The button's content. */
		children: Snippet;
		title?: string;
		align?: "start" | "end";
		buttonClass?: string;
		width?: string;
	}
	const {
		items,
		children,
		title,
		align = "start",
		buttonClass = "btn btn-ghost btn-sm",
		width = "w-72",
	}: Props = $props();

	let root = $state<HTMLDetailsElement | null>(null);

	function close() {
		if (root) root.open = false;
	}

	function run(fn: () => unknown) {
		close();
		void fn();
	}

	function onWindowClick(event: MouseEvent) {
		if (root?.open && !root.contains(event.target as Node)) close();
	}
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => e.key === "Escape" && close()} />

<details class="dropdown {align === 'end' ? 'dropdown-end' : ''}" bind:this={root}>
	<summary class="{buttonClass} list-none" {title}>{@render children()}</summary>
	<ul
		class="dropdown-content menu menu-sm bg-base-200 border-base-300 rounded-box z-40 mt-1 {width} border p-1 shadow-xl"
	>
		<MenuItems {items} onrun={run} />
	</ul>
</details>
