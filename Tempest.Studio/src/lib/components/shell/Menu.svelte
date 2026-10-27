<!--
	A dropdown menu built from command ids (shortcuts and enabled state come
	from the command registry); "-" is a separator, a `{ label, run }` object
	an ad-hoc item (recent folders…).
-->
<script module lang="ts">
	export type MenuItem =
		| string
		| { label: string; detail?: string; run: () => unknown; disabled?: boolean }
		| { heading: string };
</script>

<script lang="ts">
	import { commands, keyLabel } from "$lib/studio/commands.svelte";
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
		{#each items as item, i (i)}
			{#if item === "-"}
				<li class="border-base-300 my-1 border-t"></li>
			{:else if typeof item === "string"}
				{@const c = commands.get(item)}
				{#if c}
					<li class={commands.enabled(c) ? "" : "menu-disabled"}>
						<button
							type="button"
							disabled={!commands.enabled(c)}
							onclick={() => run(() => commands.run(c.id))}
						>
							{#if c.icon}<c.icon size={14} class="shrink-0 opacity-80" />{:else}<span
									class="w-3.5"
								></span>{/if}
							<span class="flex-1 truncate">{c.title}</span>
							{#if c.keys}<kbd class="kbd kbd-xs opacity-70">{keyLabel(c.keys)}</kbd
								>{/if}
						</button>
					</li>
				{/if}
			{:else if "heading" in item}
				<li class="menu-title text-[11px]">{item.heading}</li>
			{:else}
				<li class={item.disabled ? "menu-disabled" : ""}>
					<button
						type="button"
						disabled={item.disabled}
						title={item.detail}
						onclick={() => run(item.run)}
					>
						<span class="w-3.5"></span>
						<span class="flex-1 truncate">{item.label}</span>
					</button>
				</li>
			{/if}
		{/each}
	</ul>
</details>
