<!--
	The items of a menu (dropdown or context menu): command ids (title,
	shortcut and enabled state from the registry), "-" separators, headings
	and ad-hoc `{ label, run }` items.
-->
<script module lang="ts">
	import type { Component } from "svelte";

	export type MenuItem =
		| string
		| {
				label: string;
				detail?: string;
				run: () => unknown;
				disabled?: boolean;
				icon?: Component;
				/** Shortcut shown on the right (`Mod+D`). */
				keys?: string;
				/** Destructive: shown in the error colour. */
				danger?: boolean;
		  }
		| { heading: string };
</script>

<script lang="ts">
	import { commands, keyLabel } from "$lib/studio/commands.svelte";

	interface Props {
		items: MenuItem[];
		/** Runs an item (the menu closes first). */
		onrun: (fn: () => unknown) => void;
	}
	const { items, onrun }: Props = $props();
</script>

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
					onclick={() => onrun(() => commands.run(c.id))}
				>
					{#if c.icon}<c.icon size={14} class="shrink-0 opacity-80" />{:else}<span
							class="w-3.5"
						></span>{/if}
					<span class="flex min-w-0 flex-1 flex-col">
						<span class="truncate">{c.title}</span>
						{#if c.detail}<span class="truncate text-[11px] opacity-60">{c.detail}</span
							>{/if}
					</span>
					{#if c.keys}<kbd class="kbd kbd-xs opacity-70">{keyLabel(c.keys)}</kbd>{/if}
				</button>
			</li>
		{/if}
	{:else if "heading" in item}
		<li class="menu-title truncate text-[11px]">{item.heading}</li>
	{:else}
		<li class={item.disabled ? "menu-disabled" : ""}>
			<button
				type="button"
				class={item.danger ? "text-error" : ""}
				disabled={item.disabled}
				title={item.detail}
				onclick={() => onrun(item.run)}
			>
				{#if item.icon}<item.icon size={14} class="shrink-0 opacity-80" />{:else}<span
						class="w-3.5"
					></span>{/if}
				<span class="flex min-w-0 flex-1 flex-col">
					<span class="truncate">{item.label}</span>
					{#if item.detail}<span class="truncate text-[11px] opacity-60"
							>{item.detail}</span
						>{/if}
				</span>
				{#if item.keys}<kbd class="kbd kbd-xs opacity-70">{keyLabel(item.keys)}</kbd>{/if}
			</button>
		</li>
	{/if}
{/each}
