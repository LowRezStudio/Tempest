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
	import { inField, step } from "$lib/studio/keys";
	import { prerenderView, SWF_GROUPS, type SwfDoc, type SwfNode } from "$lib/studio/swf.svelte";
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

	// The tree is a virtual list: a movie can have thousands of characters
	// (endofmatchlobby: 1428), and rendering every row took ~110 ms each
	// time a movie was opened. Only the rows in view (and a margin) exist.
	type Item =
		| {
				kind: "row";
				node: SwfNode;
				label: string;
				icon: typeof Info;
				extra: string;
				indent: boolean;
		  }
		| { kind: "group"; g: (typeof groups)[number] }
		| { kind: "sep" };
	/** Heights in pixels: a row, a group title (its 4 px margin included), the separator. */
	const heightOf = (it: Item) => (it.kind === "row" ? 20 : it.kind === "group" ? 24 : 9);
	const OVERSCAN = 12;

	const items = $derived.by((): Item[] => {
		const out: Item[] = [
			{
				kind: "row",
				node: "movie",
				label: "Movie",
				icon: Info,
				extra: doc.info ? `${doc.info.signature} v${doc.info.version}` : "",
				indent: false,
			},
			{
				kind: "row",
				node: "frames",
				label: "Frames",
				icon: Clapperboard,
				extra: doc.tree ? `${doc.tree.rootFrames}` : "",
				indent: false,
			},
		];
		for (const g of groups) {
			out.push({ kind: "group", g });
			if (collapsed[g.id] && q === "") continue;
			const icon = groupIcons[g.id as keyof typeof groupIcons] ?? Binary;
			for (const c of g.items) {
				out.push({
					kind: "row",
					node: `char:${c.id}`,
					label:
						c.names[0] ?? `${c.kind === "edittext" ? "EditText" : c.tagName} ${c.id}`,
					icon,
					extra: c.names[0] ? String(c.id) : c.kind === "sprite" ? `${c.frames}f` : "",
					indent: true,
				});
			}
		}
		const tail: [SwfNode, string, typeof Info, string][] = [
			["texts", "Edit texts", Type, `${doc.info?.texts.length ?? ""}`],
			["scripts", "Scripts", FileCode, `${doc.info?.scripts.length ?? ""}`],
			["images", "External images", Image, `${doc.info?.images.length ?? ""}`],
			[
				"linkage",
				"Linkage",
				Link,
				doc.info ? `${doc.info.exports.length}/${doc.info.imports.length}` : "",
			],
			["tags", "All tags", Tags, `${doc.info?.tags.length ?? ""}`],
		];
		out.push({ kind: "sep" });
		for (const [node, label, icon, extra] of tail) {
			out.push({ kind: "row", node, label, icon, extra, indent: false });
		}
		return out;
	});

	/** Top of each item, and the total height at the end. */
	const offsets = $derived.by(() => {
		const o = [0];
		for (const it of items) o.push(o.at(-1)! + heightOf(it));
		return o;
	});

	/** Rows in display order, for keyboard navigation. */
	const rows = $derived(items.flatMap((it) => (it.kind === "row" ? [it.node] : [])));

	let scroller = $state<HTMLDivElement | null>(null);
	let scrollTop = $state(0);
	let viewport = $state(0);

	/** Items to render: `[from, to)`. Until the pane is measured, a screenful. */
	const range = $derived.by((): [number, number] => {
		const o = offsets;
		const view = viewport || 1000;
		let lo = 0;
		let hi = items.length;
		while (lo < hi) {
			const mid = (lo + hi) >> 1;
			if (o[mid + 1] <= scrollTop) lo = mid + 1;
			else hi = mid;
		}
		let end = lo;
		while (end < items.length && o[end] < scrollTop + view) end++;
		return [Math.max(0, lo - OVERSCAN), Math.min(items.length, end + OVERSCAN)];
	});

	/** Scroll a row into view (without reading layout), then optionally focus it. */
	function revealNode(node: SwfNode, focus: boolean) {
		const i = items.findIndex((it) => it.kind === "row" && it.node === node);
		if (i < 0 || !scroller) return;
		const top = offsets[i];
		const bottom = offsets[i + 1];
		const view = viewport || scroller.clientHeight;
		if (top < scrollTop) scroller.scrollTop = top;
		else if (bottom > scrollTop + view) scroller.scrollTop = bottom - view;
		// Scroll events only come with the next rendered frame.
		scrollTop = scroller.scrollTop;
		if (focus) {
			requestAnimationFrame(() =>
				scroller
					?.querySelector<HTMLElement>(`[data-swf-node="${node}"]`)
					?.focus({ preventScroll: true }),
			);
		}
	}

	function onKeyDown(event: KeyboardEvent) {
		if (inField(event)) return;
		if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
		event.preventDefault();
		const i = step(rows.indexOf(doc.node), event.key === "ArrowDown" ? 1 : -1, rows.length);
		if (i < 0) return;
		select(rows[i]);
		revealNode(rows[i], true);
	}

	function select(node: SwfNode) {
		if (node.startsWith("char:")) doc.openCharacter(Number(node.slice(5)));
		else doc.open(node);
	}

	// Resting on a row renders its view in the background, so the click
	// shows it at once; passing over rows on the way somewhere starts nothing.
	let hoverTimer: ReturnType<typeof setTimeout> | null = null;
	function hover(node: SwfNode | null) {
		if (hoverTimer) clearTimeout(hoverTimer);
		hoverTimer = null;
		if (node === null || node === doc.node) return;
		hoverTimer = setTimeout(() => void prerenderView(doc, node).catch(() => {}), 80);
	}
	$effect(() => () => hover(null));

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
			// After the expanded group is in `items`.
			queueMicrotask(() => revealNode(node, false));
		});
	});
</script>

{#snippet row(node: SwfNode, label: string, Icon: typeof Info, extra = "", indent = false)}
	<button
		class="flex h-5 w-full items-center gap-1.5 truncate rounded px-1.5 text-left text-xs {doc.node ===
		node
			? 'bg-primary text-primary-content'
			: 'hover:bg-base-300'} {indent ? 'pl-6' : ''}"
		data-swf-node={node}
		onclick={() => select(node)}
		onpointerenter={() => hover(node)}
		onpointerleave={() => hover(null)}
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
	<div
		class="min-h-0 flex-1 overflow-auto px-1 pb-2"
		bind:this={scroller}
		bind:clientHeight={viewport}
		onscroll={() => (scrollTop = scroller?.scrollTop ?? 0)}
		onkeydown={onKeyDown}
	>
		<div style="height: {offsets[range[0]]}px"></div>
		{#each items.slice(range[0], range[1]) as it (it.kind === "row" ? it.node : it.kind === "group" ? `g:${it.g.id}` : "sep")}
			{#if it.kind === "row"}
				{@render row(it.node, it.label, it.icon, it.extra, it.indent)}
			{:else if it.kind === "group"}
				{@const g = it.g}
				{@const Icon = groupIcons[g.id as keyof typeof groupIcons] ?? Binary}
				<button
					class="hover:bg-base-300 mt-1 flex h-5 w-full items-center gap-1 rounded px-1 text-left text-xs font-semibold"
					onclick={() => (collapsed = { ...collapsed, [g.id]: !collapsed[g.id] })}
				>
					{#if collapsed[g.id] && q === ""}<ChevronRight size={12} />{:else}<ChevronDown
							size={12}
						/>{/if}
					<Icon size={12} class="opacity-70" />
					{g.title}<span class="font-normal opacity-50">{count(g.items.length)}</span>
				</button>
			{:else}
				<div class="border-base-300 my-1 h-px border-t"></div>
			{/if}
		{/each}
		<div style="height: {offsets[items.length] - offsets[range[1]]}px"></div>
	</div>
</div>
