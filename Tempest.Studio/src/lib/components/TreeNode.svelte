<script lang="ts">
	import { ChevronDown, ChevronRight } from "@lucide/svelte";
	import ClassIcon from "$lib/components/ClassIcon.svelte";
	import TreeNode from "$lib/components/TreeNode.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import type { Tree } from "$lib/studio/tree";
	import type { ExportRow } from "../../../electron/src/ipc/contract";
	import type { SvelteSet } from "svelte/reactivity";

	interface Props {
		index: number;
		depth: number;
		rows: ExportRow[];
		tree: Tree;
		expanded: SvelteSet<number>;
	}
	const { index, depth, rows, tree, expanded }: Props = $props();

	const row = $derived(rows[index]!);
	const children = $derived(tree.children.get(index) ?? []);
	const isOpen = $derived(expanded.has(index));
	const placeholder = $derived(row.size === 0 && row.classRef === 0);

	function toggle(event: Event) {
		event.stopPropagation();
		if (isOpen) expanded.delete(index);
		else expanded.add(index);
	}

	const isPackage = $derived(row.class.toLowerCase() === "package");

	/** Packages are folders: a click opens them (and selects, for the info pane). */
	function onClick(event: MouseEvent) {
		if (isPackage && children.length > 0) toggle(event);
		void studio.select(index);
	}
</script>

<div
	class="row-hover flex w-full cursor-pointer items-center gap-1 py-0.5 pr-2 font-mono text-xs {studio.selected ===
	index
		? 'row-selected'
		: ''} {placeholder ? 'opacity-40' : ''}"
	style="padding-left: {depth * 14 + 6}px"
	data-export={index}
	role="treeitem"
	tabindex="0"
	aria-selected={studio.selected === index}
	aria-expanded={children.length > 0 ? isOpen : undefined}
	onclick={onClick}
	ondblclick={toggle}
	onkeydown={(e) => {
		// Arrows and Enter are handled by the tree; space selects.
		if (e.key === " ") {
			e.preventDefault();
			void studio.select(index);
		}
	}}
	title="{row.class} · {row.path} · {row.size} bytes"
>
	{#if children.length > 0}
		<button
			class="shrink-0 opacity-60 hover:opacity-100"
			onclick={toggle}
			tabindex="-1"
			aria-label="Toggle"
		>
			{#if isOpen}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}
		</button>
	{:else}
		<span class="w-3 shrink-0"></span>
	{/if}
	<ClassIcon className={row.class} />
	<span class="truncate">{row.name}</span>
	{#if children.length > 0}
		<span class="ml-1 shrink-0 text-[10px] opacity-40">{tree.descendants.get(index)}</span>
	{/if}
	<span class="ml-auto shrink-0 pl-2 text-[10px] opacity-50">{row.class}</span>
</div>
{#if isOpen}
	{#each children as c (c)}
		<TreeNode index={c} depth={depth + 1} {rows} {tree} {expanded} />
	{/each}
{/if}
