<!-- Maps: picker and outliner | 3D view or Kismet graph | inspector. -->
<script lang="ts">
	import {
		Box,
		Clapperboard,
		Download,
		Sun,
		Plus,
		Trash2,
		Map as MapIcon,
		Move,
		RefreshCw,
		Rotate3d,
		Scaling,
		MousePointer2,
		Workflow,
	} from "@lucide/svelte";
	import ActorPalette from "$lib/components/map/ActorPalette.svelte";
	import CollisionToolbar from "$lib/components/map/CollisionToolbar.svelte";
	import KismetGraph from "$lib/components/map/KismetGraph.svelte";
	import LightBakeDialog from "$lib/components/map/LightBakeDialog.svelte";
	import MapExportDialog from "$lib/components/map/MapExportDialog.svelte";
	import MapInspector from "$lib/components/map/MapInspector.svelte";
	import MapSide from "$lib/components/map/MapSide.svelte";
	import MapViewport from "$lib/components/map/MapViewport.svelte";
	import MatineePanel from "$lib/components/map/MatineePanel.svelte";
	import NavMeshPanel from "$lib/components/map/NavMeshPanel.svelte";
	import PropertyTree from "$lib/components/PropertyTree.svelte";
	import Pane from "$lib/components/shell/Pane.svelte";
	import { basename } from "$lib/studio/format";
	import { lightBake } from "$lib/studio/lightbake.svelte";
	import { map, type Shading } from "$lib/studio/map.svelte";
	import { navmesh } from "$lib/studio/navmesh.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { ui } from "$lib/studio/ui.svelte";
	import type { ParsedObject } from "../../../../electron/src/ipc/contract";

	const SHADINGS: { id: Shading; label: string }[] = [
		{ id: "lit", label: "Lit" },
		{ id: "unlit", label: "Unlit" },
		{ id: "untextured", label: "No textures" },
		{ id: "wireframe", label: "Wireframe" },
	];
	const TOOLS = [
		{ id: "select" as const, icon: MousePointer2, label: "Select only (Q)" },
		{ id: "translate" as const, icon: Move, label: "Move (W)" },
		{ id: "rotate" as const, icon: Rotate3d, label: "Rotate (E)" },
		{ id: "scale" as const, icon: Scaling, label: "Scale (R)" },
	];

	let exporting = $state(false);
	let graph = $state<ReturnType<typeof KismetGraph> | null>(null);

	// The Kismet node shown in the inspector.
	let nodeObject = $state<ParsedObject | null>(null);
	$effect(() => {
		const level = map.kismetLevel;
		const node = map.kismetSelected;
		nodeObject = null;
		if (!level || node === null) return;
		void rpc("object.get", { path: level, index: node })
			.then((o) => {
				if (map.kismetSelected === node) nodeObject = o;
			})
			.catch(() => {});
	});
	const selectedNode = $derived(
		map.graph?.nodes.find((n) => n.export === map.kismetSelected) ?? null,
	);
	const kismetPath = $derived.by(() => {
		const out: { export: number; name: string }[] = [];
		let cur = map.sequences.find((s) => s.export === map.sequence);
		while (cur) {
			out.unshift({ export: cur.export, name: cur.title || cur.name });
			const parent = cur.parent;
			cur = parent === null ? undefined : map.sequences.find((s) => s.export === parent);
		}
		return out;
	});
</script>

<div class="relative flex min-h-0 flex-1">
	{#if !ui.isCollapsed("side")}
		<Pane id="map.side" side="left" initial={280} min={200} max={520} overlay={ui.compact}>
			<MapSide />
		</Pane>
	{/if}
	<div class="flex min-w-0 flex-1 flex-col">
		{#if map.path}
			<div
				class="border-base-300 bg-base-200 flex min-h-9 shrink-0 flex-wrap items-center gap-1 border-b px-2 py-1 text-xs"
			>
				<div class="join">
					<button
						class="btn join-item btn-xs {map.view === 'scene' ? 'btn-active' : ''}"
						onclick={() => {
							map.view = "scene";
							map.record();
						}}><MapIcon size={12} /> Scene</button
					>
					<button
						class="btn join-item btn-xs {map.view === 'kismet' ? 'btn-active' : ''}"
						onclick={() =>
							map.openKismet(map.kismetLevel ?? map.selected?.level ?? map.path!)}
						><Workflow size={12} /> Kismet</button
					>
					{#if map.matinee && map.matineeAt}
						{@const at = map.matineeAt}
						<button
							class="btn join-item btn-xs {map.view === 'matinee'
								? 'btn-active'
								: ''}"
							onclick={() => map.openMatinee(at.level, at.index)}
							title="The Matinee timeline last opened"
							><Clapperboard size={12} /> Matinee</button
						>
					{/if}
				</div>
				{#if map.view === "scene" || map.view === "matinee"}
					<div class="join ml-2">
						{#each TOOLS as t (t.id)}
							<button
								class="btn join-item btn-xs btn-square {map.tool === t.id
									? 'btn-active'
									: ''}"
								onclick={() => (map.tool = t.id)}
								title={t.label}
								aria-label={t.label}><t.icon size={12} /></button
							>
						{/each}
					</div>
					<button
						class="btn btn-xs ml-2 {map.heavyWalls ? 'btn-active btn-error' : ''}"
						onclick={() => (map.heavyWalls = !map.heavyWalls)}
						title="Colour what can't be stood on (m_bCanBaseOn False): walls flagged like this are the heavy walls"
						>Heavy walls{#if map.heavyWalls}&nbsp;<span class="opacity-70"
								>{map.rooftopCount}</span
							>{/if}</button
					>
					{#if map.heavyWalls}
						<label
							class="ml-1 flex items-center gap-1"
							title="Draw each flagged object in one flat colour, seen through walls"
						>
							<input
								type="checkbox"
								class="checkbox checkbox-xs"
								checked={map.heavyStyle === "xray"}
								onchange={(e) =>
									(map.heavyStyle = e.currentTarget.checked ? "xray" : "slope")}
							/>
							X-ray
						</label>
					{/if}
					<CollisionToolbar />
					<button
						class="btn btn-xs ml-2 {navmesh.show ? 'btn-active btn-info' : ''}"
						onclick={() => (navmesh.show = !navmesh.show)}
						title="Draw the Havok nav mesh the bots walk on: faces by region or cluster, boundaries, jump links; pick and edit faces"
						>Nav mesh</button
					>
					<select
						class="select select-xs ml-2 w-32"
						bind:value={map.shading}
						title="Shading"
					>
						{#each SHADINGS as s (s.id)}
							<option value={s.id}>{s.label}</option>
						{/each}
					</select>
					<div class="flex-1"></div>
					<button
						class="btn btn-ghost btn-xs {map.addingActor ? 'btn-active' : ''}"
						onclick={() => (map.addingActor = !map.addingActor)}
						title="Add an actor: a copy of one any level places, where the view looks (Shift+A)"
						><Plus size={12} /> Actor</button
					>
					<button
						class="btn btn-ghost btn-xs btn-square"
						disabled={!map.actor()}
						onclick={() => map.removeSelected()}
						title="Delete the selected actor (Delete)"
						aria-label="Delete the selected actor"><Trash2 size={12} /></button
					>
					<button
						class="btn btn-ghost btn-xs"
						onclick={() => (exporting = true)}
						title="Export the map (the levels and kinds shown) as GLB or FBX"
						><Download size={12} /> Export</button
					>
					<button
						class="btn btn-ghost btn-xs"
						onclick={() => (lightBake.open = true)}
						title="Bake static light maps for edited, selected or whole levels' meshes"
						><Sun size={12} /> Light</button
					>
				{:else}
					<select
						class="select select-xs ml-2 w-44"
						value={map.kismetLevel ?? ""}
						onchange={(e) => map.openKismet(e.currentTarget.value)}
						title="Level"
					>
						{#each map.levels as l (l.path)}
							<option value={l.path}>{l.name}</option>
						{/each}
					</select>
					<div class="ml-2 flex min-w-0 items-center gap-1 truncate">
						{#each kismetPath as s, k (s.export)}
							{#if k > 0}<span class="opacity-40">›</span>{/if}
							<button
								class="link link-hover font-mono"
								onclick={() => map.openKismet(map.kismetLevel!, s.export)}
								>{s.name}</button
							>
						{/each}
					</div>
					<div class="flex-1"></div>
					<button
						class="btn btn-ghost btn-xs"
						onclick={() => graph?.openPalette()}
						title="Add a node (A, or right-click the graph)"
						><Plus size={12} /> Node</button
					>
					<span class="opacity-50">{map.graph?.nodes.length ?? 0} nodes</span>
				{/if}
				<button
					class="btn btn-ghost btn-xs btn-square"
					onclick={() => {
						const p = map.path!;
						map.path = null;
						void map.open(p);
					}}
					title="Read the map again"><RefreshCw size={12} /></button
				>
			</div>
		{/if}
		{#if map.loading}
			<div class="flex flex-1 items-center justify-center gap-2 text-sm opacity-70">
				<span class="loading loading-spinner"></span> Reading the map and its sublevels…
			</div>
		{:else if map.error}
			<div class="text-error flex flex-1 items-center justify-center p-6 text-sm">
				{map.error}
			</div>
		{:else if !map.path}
			<div class="flex flex-1 flex-col items-center justify-center gap-2 text-sm opacity-70">
				<Box size={32} />
				Pick a map on the left.
			</div>
		{:else if map.view === "kismet"}
			<KismetGraph bind:this={graph} />
		{:else}
			<div class="relative flex min-h-0 flex-1 flex-col">
				<MapViewport />
				{#if map.addingActor}<ActorPalette />{/if}
				{#if navmesh.show}<NavMeshPanel />{/if}
			</div>
			{#if map.view === "matinee"}<MatineePanel />{/if}
		{/if}
	</div>
	{#if map.path && !ui.isCollapsed("details")}
		<Pane
			id="map.inspector"
			side="right"
			initial={360}
			min={260}
			max={800}
			overlay={ui.compact}
		>
			{#if map.view === "kismet"}
				<div class="flex h-full min-h-0 flex-col text-xs">
					{#if selectedNode && map.kismetLevel}
						<div class="border-base-300 space-y-1 border-b p-2">
							<div class="font-mono text-sm font-semibold">
								{selectedNode.title || selectedNode.name}
							</div>
							<div class="flex flex-wrap gap-x-3 opacity-70">
								<span class="font-mono">{selectedNode.class}</span>
								<span>{selectedNode.kind}</span>
								<span>export {selectedNode.export}</span>
							</div>
							{#if selectedNode.comment}<p class="opacity-70">
									{selectedNode.comment}
								</p>{/if}
							{#if selectedNode.originator?.export != null}
								{@const o = selectedNode.originator}
								<button
									class="link link-hover"
									onclick={() => {
										map.view = "scene";
										void map.select({
											level: map.kismetLevel!,
											export: o.export!,
										});
										map.focusRequest++;
									}}>Fired by {o.path.split(".").at(-1)}: show it</button
								>
							{/if}
						</div>
						<div class="min-h-0 flex-1 overflow-auto">
							{#if nodeObject?.kind === "object"}
								<PropertyTree
									obj={nodeObject}
									path={map.kismetLevel}
									onchange={(u) => {
										nodeObject = u;
										void map.loadGraph();
									}}
								/>
							{/if}
						</div>
					{:else}
						<p class="p-4 opacity-60">
							Select a node to see and edit its properties. The node positions are
							rebuilt from the connectors the cooker kept; moving a node only changes
							this view.
						</p>
					{/if}
				</div>
			{:else}
				<MapInspector />
			{/if}
		</Pane>
	{/if}
</div>

<MapExportDialog bind:open={exporting} />
<LightBakeDialog />
