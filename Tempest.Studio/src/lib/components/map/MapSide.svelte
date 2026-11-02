<!-- The map view's side bar: which map, its levels (show/hide, Kismet), what
	kinds of actors are drawn, and the outliner. -->
<script lang="ts">
	import { Eye, EyeOff, Map as MapIcon, Workflow } from "@lucide/svelte";
	import { basename } from "$lib/studio/format";
	import { CATEGORIES, categoryColor, map, TEAM_COLORS } from "$lib/studio/map.svelte";
	import { studio } from "$lib/studio/state.svelte";

	let mapFilter = $state("");

	const maps = $derived(
		(map.list ?? []).filter(
			(m) =>
				(!map.persistentOnly || m.persistent) &&
				m.name.toLowerCase().includes(mapFilter.trim().toLowerCase()),
		),
	);

	type Row = {
		level: string;
		export: number;
		name: string;
		class: string;
		category: string;
		team?: number;
	};
	const rows = $derived.by(() => {
		const q = map.filter.trim().toLowerCase();
		const out: Row[] = [];
		for (const l of map.levels) {
			if (map.hiddenLevels[l.path]) continue;
			for (const a of l.actors) {
				if (map.rooftopsOnly) {
					if (a.canBaseOn !== false) continue;
				} else if (map.hiddenCategories[a.category]) continue;
				if (q && !a.name.toLowerCase().includes(q) && !a.class.toLowerCase().includes(q)) {
					continue;
				}
				const team = Number(a.info.TeamIndex);
				out.push({
					level: l.path,
					export: a.export,
					name: a.name,
					class: a.class,
					category: a.category,
					team: Number.isFinite(team) ? team : undefined,
				});
			}
		}
		return out;
	});
	const totals = $derived.by(() => {
		const t: Record<string, number> = {};
		for (const l of map.levels) {
			for (const a of l.actors) t[a.category] = (t[a.category] ?? 0) + 1;
		}
		return t;
	});
	const LIMIT = 1500;

	$effect(() => {
		if (studio.workspace && !map.list && !map.listLoading) void map.loadList();
	});
</script>

<div class="flex h-full min-h-0 flex-col text-xs">
	{#if !map.path}
		<div class="border-base-300 flex items-center gap-2 border-b p-2">
			<input
				class="input input-xs flex-1"
				placeholder="Filter maps…"
				bind:value={mapFilter}
			/>
		</div>
		<label class="flex items-center gap-2 px-2 py-1 opacity-70">
			<input type="checkbox" class="checkbox checkbox-xs" bind:checked={map.persistentOnly} />
			Playable maps only (those that stream sublevels)
		</label>
		<div class="min-h-0 flex-1 overflow-auto">
			{#if map.listLoading}
				<div class="flex items-center gap-2 p-2 opacity-60">
					<span class="loading loading-xs loading-spinner"></span> Looking for levels…
				</div>
			{/if}
			{#each maps as m (m.path)}
				<button
					class="hover:bg-base-300 flex w-full items-center gap-2 px-2 py-1 text-left"
					onclick={() => map.open(m.path)}
				>
					<MapIcon size={12} class="shrink-0 opacity-60" />
					<span class="flex-1 truncate font-mono">{m.name.replace(/\.upk$/i, "")}</span>
					{#if m.kismet}<span title="Has Kismet"
							><Workflow size={11} class="opacity-40" /></span
						>{/if}
				</button>
			{/each}
		</div>
	{:else}
		<div class="border-base-300 flex items-center gap-1 border-b px-2 py-1.5">
			<MapIcon size={13} class="opacity-60" />
			<span class="flex-1 truncate font-mono font-semibold">{basename(map.path)}</span>
			<button
				class="btn btn-ghost btn-xs"
				onclick={() => {
					map.path = null;
					map.levels = [];
					map.selected = null;
				}}
				title="Pick another map">Change</button
			>
		</div>
		<details class="border-base-300 border-b" open>
			<summary class="cursor-pointer px-2 py-1 font-semibold uppercase opacity-60"
				>Levels</summary
			>
			{#each map.levels as l (l.path)}
				{@const hidden = map.hiddenLevels[l.path]}
				<div class="flex items-center gap-1 px-2 py-0.5 {hidden ? 'opacity-50' : ''}">
					<button
						class="btn btn-ghost btn-xs btn-square h-5 min-h-5 w-5"
						onclick={() =>
							(map.hiddenLevels = { ...map.hiddenLevels, [l.path]: !hidden })}
						title={hidden ? "Show this level" : "Hide this level"}
					>
						{#if hidden}<EyeOff size={11} />{:else}<Eye size={11} />{/if}
					</button>
					<span class="flex-1 truncate font-mono" title={l.path}
						>{l.name}{#if studio.dirty.includes(l.path)}<span class="text-warning">
								●</span
							>{/if}</span
					>
					<span class="opacity-40">{l.actors.length}</span>
					<button
						class="btn btn-ghost btn-xs btn-square h-5 min-h-5 w-5"
						onclick={() => map.openKismet(l.path)}
						title="Kismet of this level"
					>
						<Workflow size={11} />
					</button>
				</div>
			{/each}
			{#if map.missing.length > 0}
				<div class="text-warning px-2 py-1">
					Not in this folder: {map.missing.join(", ")}
				</div>
			{/if}
		</details>
		<details class="border-base-300 border-b">
			<summary class="cursor-pointer px-2 py-1 font-semibold uppercase opacity-60"
				>Show</summary
			>
			<div class="grid grid-cols-2 gap-x-2 px-2 pb-1">
				{#each CATEGORIES as c (c.id)}
					{#if totals[c.id]}
						<label class="flex items-center gap-1.5 py-0.5">
							<input
								type="checkbox"
								class="checkbox checkbox-xs"
								checked={!map.hiddenCategories[c.id]}
								onchange={(e) =>
									(map.hiddenCategories = {
										...map.hiddenCategories,
										[c.id]: !e.currentTarget.checked,
									})}
							/>
							<span class="h-2 w-2 shrink-0 rounded-full" style="background:{c.color}"
							></span>
							<span class="truncate">{c.label}</span>
							<span class="ml-auto opacity-40">{totals[c.id]}</span>
						</label>
					{/if}
				{/each}
			</div>
		</details>
		<div class="flex items-center gap-2 p-2">
			<input
				class="input input-xs flex-1"
				placeholder="Filter actors…"
				bind:value={map.filter}
			/>
			<span class="opacity-50">{rows.length}</span>
		</div>
		<div class="min-h-0 flex-1 overflow-auto" role="listbox" aria-label="Actors">
			{#each rows.slice(0, LIMIT) as r (`${r.level}#${r.export}`)}
				{@const active =
					map.selected?.level === r.level && map.selected.export === r.export}
				<button
					class="flex w-full items-center gap-1.5 px-2 py-0.5 text-left [content-visibility:auto] {active
						? 'bg-primary/20'
						: 'hover:bg-base-300'}"
					role="option"
					aria-selected={active}
					onclick={() => map.select({ level: r.level, export: r.export })}
					ondblclick={() => map.focusRequest++}
					title="{r.class} in {basename(r.level)}"
				>
					<span
						class="h-2 w-2 shrink-0 rounded-full"
						style="background:{r.team !== undefined && r.category === 'playerstart'
							? (TEAM_COLORS[r.team] ?? categoryColor(r.category))
							: categoryColor(r.category)}"
					></span>
					<span class="flex-1 truncate font-mono">{r.name}</span>
					<span class="truncate opacity-40">{r.class}</span>
				</button>
			{/each}
			{#if rows.length > LIMIT}
				<p class="p-2 opacity-60">{rows.length - LIMIT} more: filter to find them.</p>
			{/if}
		</div>
	{/if}
</div>
