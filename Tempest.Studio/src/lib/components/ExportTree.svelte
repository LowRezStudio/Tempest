<script lang="ts">
	import { Info } from "@lucide/svelte";
	import { SvelteSet } from "svelte/reactivity";
	import ClassIcon from "$lib/components/ClassIcon.svelte";
	import PackageInfoDialog from "$lib/components/PackageInfoDialog.svelte";
	import TreeNode from "$lib/components/TreeNode.svelte";
	import { inField, reveal, step } from "$lib/studio/keys";
	import { studio } from "$lib/studio/state.svelte";
	import { ancestors, buildTree, classCounts } from "$lib/studio/tree";

	let filter = $state("");
	let classFilter = $state("");
	let infoOpen = $state(false);
	const expanded = new SvelteSet<number>();

	const rows = $derived(studio.pkg?.exports ?? []);
	const tree = $derived(buildTree(rows));
	const classes = $derived(classCounts(rows));

	// Reset the expansion state when another package is opened.
	let lastPath = $state<string | null>(null);
	$effect(() => {
		const path = studio.pkg?.path ?? null;
		if (path !== lastPath) {
			lastPath = path;
			expanded.clear();
			filter = "";
			classFilter = "";
		}
	});

	// Reveal the selection when it was chosen from outside the tree (search,
	// reference links).
	$effect(() => {
		const i = studio.selected;
		if (i === null) return;
		for (const a of ancestors(rows, i)) expanded.add(a);
		requestAnimationFrame(() => {
			document.querySelector(`[data-export="${i}"]`)?.scrollIntoView({ block: "nearest" });
		});
	});

	/** The rows on screen, top to bottom (expanded nodes only). */
	function visibleRows(): number[] {
		if (filtering) return flat;
		const out: number[] = [];
		const walk = (i: number) => {
			out.push(i);
			if (expanded.has(i)) for (const c of tree.children.get(i) ?? []) walk(c);
		};
		for (const r of tree.roots) walk(r);
		return out;
	}

	function go(i: number) {
		void studio.select(i);
		reveal(`[data-export="${i}"]`);
	}

	function onKey(event: KeyboardEvent) {
		if (inField(event) || event.altKey || event.ctrlKey || event.metaKey) return;
		const sel = studio.selected;
		switch (event.key) {
			case "ArrowDown":
			case "ArrowUp": {
				const order = visibleRows();
				const at = sel === null ? -1 : order.indexOf(sel);
				const next = order[step(at, event.key === "ArrowDown" ? 1 : -1, order.length)];
				if (next !== undefined) go(next);
				break;
			}
			case "Home":
			case "End": {
				const order = visibleRows();
				const next = order[event.key === "Home" ? 0 : order.length - 1];
				if (next !== undefined) go(next);
				break;
			}
			case "ArrowRight": {
				if (sel === null || filtering) return;
				const children = tree.children.get(sel) ?? [];
				if (children.length === 0) return;
				if (expanded.has(sel)) go(children[0]!);
				else expanded.add(sel);
				break;
			}
			case "ArrowLeft": {
				if (sel === null || filtering) return;
				if (expanded.has(sel)) {
					expanded.delete(sel);
				} else {
					const parent = rows[sel]!.outer - 1;
					if (parent >= 0 && parent < rows.length) go(parent);
				}
				break;
			}
			case "Enter": {
				if (sel === null || filtering) return;
				if (expanded.has(sel)) expanded.delete(sel);
				else if ((tree.children.get(sel) ?? []).length > 0) expanded.add(sel);
				break;
			}
			default: {
				return;
			}
		}
		event.preventDefault();
	}

	const filtering = $derived(filter.trim() !== "" || classFilter !== "");
	const MAX_FLAT = 3000;
	const flat = $derived.by(() => {
		if (!filtering) return [];
		const q = filter.trim().toLowerCase();
		const out: number[] = [];
		for (const row of rows) {
			if (classFilter && row.class !== classFilter) continue;
			if (q && !row.path.toLowerCase().includes(q)) continue;
			out.push(row.i);
			if (out.length >= MAX_FLAT) break;
		}
		return out;
	});
</script>

<div class="pane-title">
	Exports
	{#if studio.pkg}
		<span class="truncate font-mono normal-case opacity-80">{studio.pkg.name}</span>
		<button
			class="btn btn-square btn-ghost btn-xs ml-auto"
			onclick={() => (infoOpen = true)}
			title="Package info"
		>
			<Info size={13} />
		</button>
	{/if}
</div>
{#if studio.pkg}
	<div class="border-base-300 flex gap-1 border-b p-2">
		<input
			class="input input-sm flex-1 font-mono text-xs"
			placeholder="Filter by path…"
			bind:value={filter}
		/>
		<select class="select select-sm w-36 text-xs" bind:value={classFilter}>
			<option value="">All classes</option>
			{#each classes as c (c.name)}
				<option value={c.name}>{c.name} ({c.count})</option>
			{/each}
		</select>
	</div>
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div class="min-h-0 flex-1 overflow-auto py-1" role="tree" tabindex="-1" onkeydown={onKey}>
		{#if filtering}
			{#each flat as i (i)}
				{@const row = rows[i]!}
				<button
					class="row-hover flex w-full items-center gap-1.5 px-2 py-0.5 text-left font-mono text-xs {studio.selected ===
					i
						? 'row-selected'
						: ''}"
					data-export={i}
					onclick={() => studio.select(i)}
					title="{row.class} · {row.path}"
				>
					<ClassIcon className={row.class} />
					<span class="truncate">{row.path}</span>
					<span class="ml-auto shrink-0 text-[10px] opacity-50">{row.class}</span>
				</button>
			{/each}
			{#if flat.length >= MAX_FLAT}
				<div class="px-3 py-1 text-xs opacity-60">
					first {MAX_FLAT} matches shown; narrow the filter
				</div>
			{:else if flat.length === 0}
				<div class="px-3 py-1 text-xs opacity-60">no match</div>
			{/if}
		{:else}
			{#each tree.roots as i (i)}
				<TreeNode index={i} depth={0} {rows} {tree} {expanded} />
			{/each}
		{/if}
	</div>
	<PackageInfoDialog bind:open={infoOpen} />
{:else}
	<div class="flex flex-1 items-center justify-center text-sm opacity-50">Open a package</div>
{/if}
