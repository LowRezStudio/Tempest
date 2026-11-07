<!--
	The nav mesh overlay's controls (toolbar "Nav mesh"): what the persistent
	level's Havok nav mesh holds, how it is coloured, picking faces or a
	vertex in the view, and the edits (delete faces, cut regions, move a
	vertex), each one undo step of the level.
-->
<script lang="ts">
	import { map } from "$lib/studio/map.svelte";
	import { navmesh } from "$lib/studio/navmesh.svelte";

	$effect(() => {
		// Read again when the map changes or one of its levels is reloaded (undo).
		void map.path;
		void map.version;
		void navmesh.load();
	});

	const d = $derived(navmesh.data);
	let target = $state<[number, number, number]>([0, 0, 0]);
	$effect(() => {
		const v = navmesh.vertex;
		const data = navmesh.data;
		if (v === null || !data) return;
		// View space (metres, Y up) → Unreal units.
		const p = data.positions;
		target = [p[v * 3] * 100, p[v * 3 + 2] * 100, p[v * 3 + 1] * 100].map(
			(x) => Math.round(x * 100) / 100,
		) as [number, number, number];
	});
	const regionsPicked = $derived(
		d ? new Set([...navmesh.selected].map((f) => d.faceRegion[f])).size : 0,
	);
</script>

<div
	class="bg-base-200/90 absolute top-2 right-2 z-10 w-64 space-y-2 rounded p-2 text-xs shadow"
	role="region"
	aria-label="Nav mesh"
>
	<div class="flex items-center gap-2">
		<span class="font-semibold">Havok nav mesh</span>
		{#if navmesh.loading}<span class="loading loading-spinner loading-xs"></span>{/if}
		<div class="flex-1"></div>
		<button
			class="btn btn-ghost btn-xs btn-square"
			onclick={() => (navmesh.show = false)}
			title="Hide the nav mesh"
			aria-label="Hide the nav mesh">✕</button
		>
	</div>
	{#if navmesh.error}
		<p class="opacity-70">{navmesh.error}</p>
	{:else if d}
		{#if navmesh.actors.length > 1}
			<select
				class="select select-xs w-full"
				value={navmesh.current}
				onchange={(e) => {
					navmesh.current = Number(e.currentTarget.value);
					navmesh.clearSelection();
				}}
			>
				{#each navmesh.actors as a, k (a.info.index)}
					<option value={k}>{a.info.path.split(".").at(-1)} ({a.info.faces} faces)</option
					>
				{/each}
			</select>
		{/if}
		<div class="grid grid-cols-2 gap-x-2 font-mono opacity-80">
			<span>{d.info.faces} faces</span><span>{d.info.vertices} vertices</span>
			<span>{d.info.edges} edges</span><span>{d.info.boundary} boundary</span>
			<span>{d.info.regions} regions</span><span>{d.info.clusters} clusters</span>
			<span>{d.info.linkInfo.length} jump links</span><span
				title="Character radius the mesh was eroded by (Havok units: Unreal / 16)"
				>erosion {d.info.erosionRadius}</span
			>
		</div>
		<div class="flex flex-wrap items-center gap-2">
			<label class="flex items-center gap-1"
				>Colour
				<select class="select select-xs" bind:value={navmesh.colour}>
					<option value="region">by region</option>
					<option value="cluster">by cluster</option>
				</select>
			</label>
			<label class="flex items-center gap-1" title="Draw the nav mesh through the level">
				<input type="checkbox" class="checkbox checkbox-xs" bind:checked={navmesh.xray} />
				X-ray
			</label>
		</div>
		<div class="flex items-center gap-2 opacity-70">
			<span class="h-0.5 w-3" style="background:#f38ba8"></span> boundary
			<span class="h-0.5 w-3" style="background:#89dceb"></span> user edges
			<span class="h-0.5 w-3" style="background:#94e2d5"></span> jump links
		</div>
		<div class="join w-full">
			{#each [["off", "Actors"], ["faces", "Faces"], ["vertex", "Vertex"]] as [id, label] (id)}
				<button
					class="btn join-item btn-xs flex-1 {navmesh.pick === id ? 'btn-active' : ''}"
					onclick={() => (navmesh.pick = id as typeof navmesh.pick)}
					title={id === "off"
						? "Clicks select actors"
						: id === "faces"
							? "Clicks pick nav mesh faces (Shift adds)"
							: "Clicks pick the vertex of the face nearest the click"}
					>{label}</button
				>
			{/each}
		</div>
		{#if navmesh.pick === "faces"}
			<div class="space-y-1">
				<div class="opacity-70">
					{navmesh.selected.size} faces picked{navmesh.selected.size
						? `, ${regionsPicked} regions`
						: ""}
				</div>
				<div class="flex flex-wrap gap-1">
					<button
						class="btn btn-xs"
						disabled={navmesh.selected.size === 0}
						onclick={() => navmesh.selectRegions()}
						title="Pick every face of the picked faces' regions">Whole regions</button
					>
					<button
						class="btn btn-xs"
						disabled={navmesh.selected.size === 0}
						onclick={() => navmesh.clearSelection()}>Clear</button
					>
					<button
						class="btn btn-xs btn-error"
						disabled={navmesh.selected.size === 0}
						onclick={() => navmesh.deleteSelected()}
						title="Delete the picked faces: their neighbours' edges become boundaries, jump links to them go"
					>
						Delete faces</button
					>
					<button
						class="btn btn-xs btn-error"
						disabled={navmesh.selected.size === 0}
						onclick={() => navmesh.cutRegions()}
						title="Delete every face connected to the picked ones">Cut region</button
					>
				</div>
			</div>
		{:else if navmesh.pick === "vertex"}
			{#if navmesh.vertex !== null}
				{@const v = navmesh.vertex}
				<div class="space-y-1">
					<div class="opacity-70">Vertex {v} (Unreal units)</div>
					<div class="flex gap-1">
						{#each [0, 1, 2] as k (k)}
							<input
								class="input input-xs w-full font-mono"
								type="number"
								step="1"
								bind:value={target[k]}
								aria-label={"XYZ"[k]}
							/>
						{/each}
					</div>
					<button
						class="btn btn-xs w-full"
						onclick={() =>
							navmesh.moveVertex(v, [
								target[0] / 100,
								target[2] / 100,
								target[1] / 100,
							])}>Move the vertex</button
					>
				</div>
			{:else}
				<p class="opacity-60">Click a face to pick its nearest vertex.</p>
			{/if}
		{/if}
		<p class="opacity-50">
			Edits rebuild the face AABB tree and drop the clearance cache seeding; the cluster graph
			and the baked game data are left as they are. Undo with Ctrl+Z, save the level to keep
			them.
		</p>
	{/if}
</div>
