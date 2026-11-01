<!--
	Find an object anywhere in the game folder (the search index) and pick it:
	type part of its path; `isA` keeps a class and its subclasses (Texture,
	MaterialInterface, StaticMesh…). The open package's own objects of the
	`local` classes come first, so objects made in the Studio (not indexed
	yet) can be picked too.
-->
<script lang="ts">
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import type { SearchHit } from "../../../electron/src/ipc/contract";

	interface Props {
		/** A class whose objects (and subclasses') are listed. */
		isA?: string;
		/** Lower-case class names of the open package's objects to offer too (`*`: any). */
		local?: string[];
		placeholder?: string;
		/** The picked object, highlighted. */
		value?: SearchHit | null;
		onpick: (hit: SearchHit) => void;
	}
	let {
		isA,
		local = [],
		placeholder = "type part of a name or path",
		value = null,
		onpick,
	}: Props = $props();

	let query = $state("");
	let hits = $state<SearchHit[]>([]);
	let truncated = $state(false);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let timer: ReturnType<typeof setTimeout> | null = null;

	const here = $derived.by<SearchHit[]>(() => {
		const pkg = studio.pkg;
		if (!pkg || local.length === 0) return [];
		const q = query.trim().toLowerCase();
		return pkg.exports
			.filter(
				(e) =>
					(local.includes("*") || local.includes(e.class.toLowerCase())) &&
					(!q || e.path.toLowerCase().includes(q)),
			)
			.slice(0, 50)
			.map((e) => ({
				package: pkg.name,
				path: pkg.path,
				index: e.i,
				object: e.path,
				class: e.class,
			}));
	});

	async function search() {
		const q = query.trim();
		if (q.length < 2 && !isA) {
			hits = [];
			return;
		}
		busy = true;
		error = null;
		try {
			const r = await rpc("workspace.search", { query: q, isA, limit: 200 });
			const open = studio.pkg?.path;
			hits = r.results.filter((h) => h.path !== open || local.length === 0);
			truncated = r.truncated;
		} catch (error_) {
			error = error_ instanceof Error ? error_.message : String(error_);
		} finally {
			busy = false;
		}
	}

	$effect(() => {
		void query;
		if (timer) clearTimeout(timer);
		timer = setTimeout(() => void search(), 250);
		return () => {
			if (timer) clearTimeout(timer);
		};
	});

	const same = (a: SearchHit | null, b: SearchHit) =>
		!!a && a.path === b.path && a.index === b.index;
</script>

<div class="flex min-h-0 flex-col gap-1">
	<input class="input input-sm w-full font-mono" bind:value={query} {placeholder} />
	<div class="border-base-300 h-56 overflow-auto rounded border text-xs">
		{#each [...here, ...hits] as h, k (`${k}:${h.path}#${h.index}`)}
			<button
				type="button"
				class="hover:bg-base-200 flex w-full items-baseline gap-2 px-2 py-1 text-left {same(
					value,
					h,
				)
					? 'bg-primary/15'
					: ''}"
				onclick={() => onpick(h)}
				title="{h.package} · {h.object}"
			>
				<span class="min-w-0 flex-1 truncate font-mono">{h.object}</span>
				<span class="shrink-0 opacity-50">{h.class}</span>
				<span class="max-w-40 shrink-0 truncate opacity-50"
					>{k < here.length ? "this package" : h.package}</span
				>
			</button>
		{/each}
		{#if busy}
			<p class="p-2 opacity-60">Searching… (the first search indexes the game folder)</p>
		{:else if here.length + hits.length === 0}
			<p class="p-2 opacity-60">
				{error ??
					(query.trim().length < 2 && !isA
						? "Type at least 2 letters."
						: "Nothing found.")}
			</p>
		{:else if truncated}
			<p class="p-2 opacity-60">More results: type more of the name.</p>
		{/if}
	</div>
</div>
