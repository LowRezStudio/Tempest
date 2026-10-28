<!--
	The movie editor's resource tree, after FFDec's: the movie, its root
	frames, the characters by kind (linkage name or id), then the tag views
	(texts, scripts, images, linkage, every tag). A filter matches ids and
	names; Up/Down walk the visible rows.
-->
<script lang="ts" module>
	import {
		Binary,
		CaseSensitive,
		Film,
		Image,
		Import,
		MousePointerClick,
		Shapes,
		Type,
	} from "@lucide/svelte";

	/** Icon of each character group (`SWF_GROUPS` ids). */
	export const groupIcons = {
		sprites: Film,
		shapes: Shapes,
		images: Image,
		texts: Type,
		fonts: CaseSensitive,
		buttons: MousePointerClick,
		others: Binary,
		imports: Import,
	} as const;
</script>

<script lang="ts">
	import {
		ChevronDown,
		ChevronRight,
		Clapperboard,
		FileCode,
		Info,
		Link,
		Tags,
	} from "@lucide/svelte";
	import { untrack } from "svelte";
	import { inField, reveal, step } from "$lib/studio/keys";
	import { SWF_GROUPS, type SwfDoc, type SwfNode } from "$lib/studio/swf.svelte";
	import type { SwfCharacter } from "../../../../electron/src/ipc/contract";

	interface Props {
		doc: SwfDoc;
	}
	const { doc }: Props = $props();

	let collapsed = $state<Record<string, boolean>>({ imports: true, others: true });

	const q = $derived(doc.treeFilter.trim().toLowerCase());
	const matches = (c: SwfCharacter) =>
		q === "" ||
		String(c.id) === q ||
		c.names.some((n) => n.toLowerCase().includes(q)) ||
		c.kind.includes(q);

	const groups = $derived(
		SWF_GROUPS.map((g) => ({
			...g,
			items: (doc.tree?.characters ?? []).filter(
				(c) => g.kinds.includes(c.kind) && matches(c),
			),
		})).filter((g) => g.items.length > 0),
	);

	/** Rows in display order, for keyboard navigation. */
	const rows = $derived.by((): SwfNode[] => {
		const out: SwfNode[] = ["movie", "frames"];
		for (const g of groups) {
			if (collapsed[g.id] && q === "") continue;
			for (const c of g.items) out.push(`char:${c.id}`);
		}
		out.push("texts", "scripts", "images", "linkage", "tags");
		return out;
	});

	function onKeyDown(event: KeyboardEvent) {
		if (inField(event)) return;
		if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
		event.preventDefault();
		const i = step(rows.indexOf(doc.node), event.key === "ArrowDown" ? 1 : -1, rows.length);
		if (i < 0) return;
		select(rows[i]);
		reveal(`[data-swf-node="${rows[i]}"]`);
	}

	function select(node: SwfNode) {
		if (node.startsWith("char:")) doc.openCharacter(Number(node.slice(5)));
		else doc.open(node);
	}

	const count = (n: number | undefined) => (n === undefined ? "" : ` (${n})`);

	// Follow the editor: entering a clip on the stage or a breadcrumb opens
	// its group and scrolls the tree to it.
	// Only a new node counts: edits refresh the dictionary, and collapsing
	// the group by hand must stay possible.
	$effect(() => {
		const node = doc.node;
		if (!node.startsWith("char:")) return;
		untrack(() => {
			const c = doc.chars.get(Number(node.slice(5)));
			const g = c && SWF_GROUPS.find((x) => x.kinds.includes(c.kind));
			if (g && collapsed[g.id]) collapsed = { ...collapsed, [g.id]: false };
			reveal(`[data-swf-node="${node}"]`, false);
		});
	});
</script>

{#snippet row(node: SwfNode, label: string, Icon: typeof Info, extra = "", indent = false)}
	<button
		class="flex w-full items-center gap-1.5 truncate rounded px-1.5 py-0.5 text-left text-xs {doc.node ===
		node
			? 'bg-primary text-primary-content'
			: 'hover:bg-base-300'} {indent ? 'pl-6' : ''}"
		data-swf-node={node}
		onclick={() => select(node)}
		title={label}
	>
		<Icon size={12} class="shrink-0 opacity-70" />
		<span class="truncate">{label}</span>
		{#if extra}<span class="ml-auto shrink-0 font-mono text-[10px] opacity-50">{extra}</span
			>{/if}
	</button>
{/snippet}

<div class="flex h-full min-h-0 flex-col">
	<div class="p-1">
		<input
			class="input input-xs w-full"
			placeholder="Filter by id or name…"
			bind:value={doc.treeFilter}
		/>
	</div>
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="min-h-0 flex-1 overflow-auto px-1 pb-2" onkeydown={onKeyDown}>
		{@render row(
			"movie",
			"Movie",
			Info,
			doc.info ? `${doc.info.signature} v${doc.info.version}` : "",
		)}
		{@render row("frames", "Frames", Clapperboard, doc.tree ? `${doc.tree.rootFrames}` : "")}
		{#each groups as g (g.id)}
			{@const Icon = groupIcons[g.id as keyof typeof groupIcons] ?? Binary}
			<button
				class="hover:bg-base-300 mt-1 flex w-full items-center gap-1 rounded px-1 py-0.5 text-left text-xs font-semibold"
				onclick={() => (collapsed = { ...collapsed, [g.id]: !collapsed[g.id] })}
			>
				{#if collapsed[g.id] && q === ""}<ChevronRight size={12} />{:else}<ChevronDown
						size={12}
					/>{/if}
				<Icon size={12} class="opacity-70" />
				{g.title}<span class="font-normal opacity-50">{count(g.items.length)}</span>
			</button>
			{#if !collapsed[g.id] || q !== ""}
				{#each g.items as c (c.id)}
					{@render row(
						`char:${c.id}`,
						c.names[0] ?? `${c.kind === "edittext" ? "EditText" : c.tagName} ${c.id}`,
						Icon,
						c.names[0] ? String(c.id) : c.kind === "sprite" ? `${c.frames}f` : "",
						true,
					)}
				{/each}
			{/if}
		{/each}
		<div class="border-base-300 mt-1 border-t pt-1">
			{@render row("texts", "Edit texts", Type, `${doc.info?.texts.length ?? ""}`)}
			{@render row("scripts", "Scripts", FileCode, `${doc.info?.scripts.length ?? ""}`)}
			{@render row("images", "External images", Image, `${doc.info?.images.length ?? ""}`)}
			{@render row(
				"linkage",
				"Linkage",
				Link,
				doc.info ? `${doc.info.exports.length}/${doc.info.imports.length}` : "",
			)}
			{@render row("tags", "All tags", Tags, `${doc.info?.tags.length ?? ""}`)}
		</div>
	</div>
</div>
