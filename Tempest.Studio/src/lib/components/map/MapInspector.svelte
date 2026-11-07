<!-- The selected actor: where it is, what it draws, and its properties
	(edited in its own level package; a change that moves it redraws it). -->
<script lang="ts">
	import { Copy, ExternalLink, Focus, Trash2 } from "@lucide/svelte";
	import CollisionPanel from "$lib/components/map/CollisionPanel.svelte";
	import PropertyTree from "$lib/components/PropertyTree.svelte";
	import { basename } from "$lib/studio/format";
	import { categoryColor, map } from "$lib/studio/map.svelte";
	import { studio } from "$lib/studio/state.svelte";

	const actor = $derived(map.actor());
	const level = $derived(map.selected?.level ?? null);
	const fmt = (v: number | undefined) =>
		v === undefined ? "–" : String(Math.round(v * 100) / 100);
</script>

<div class="flex h-full min-h-0 flex-col text-xs">
	{#if !actor || !level}
		<p class="p-4 opacity-60">
			Click an actor in the view or the list. Drag the gizmo to move it (W), rotate it (E) or
			scale it (R); the change is written to its level package like any other edit. Shift+A
			adds an actor, Ctrl+D duplicates the selected one, Delete deletes it.
		</p>
	{:else}
		<div class="border-base-300 space-y-1 border-b p-2">
			<div class="flex items-center gap-2">
				<span
					class="h-2.5 w-2.5 shrink-0 rounded-full"
					style="background:{categoryColor(actor.category)}"
				></span>
				<span class="flex-1 truncate font-mono text-sm font-semibold">{actor.name}</span>
				<button
					class="btn btn-ghost btn-xs btn-square"
					onclick={() => map.focusRequest++}
					title="Frame it (F)"><Focus size={12} /></button
				>
				<button
					class="btn btn-ghost btn-xs btn-square"
					onclick={() => map.duplicate()}
					title="Duplicate it next to itself (Ctrl+D)"><Copy size={12} /></button
				>
				<button
					class="btn btn-ghost btn-xs btn-square hover:text-error"
					onclick={() => map.removeSelected()}
					title="Delete it (Delete)"><Trash2 size={12} /></button
				>
				<button
					class="btn btn-ghost btn-xs btn-square"
					onclick={() => studio.visit(level, actor.export)}
					title="Open it in the package view"><ExternalLink size={12} /></button
				>
			</div>
			<div class="flex flex-wrap gap-x-3 opacity-70">
				<span class="font-mono">{actor.class}</span>
				<span>{basename(level)}</span>
				<span>export {actor.export}</span>
			</div>
			{#if actor.edit === "component"}
				<p class="opacity-60">
					A mesh the cooker gathered into a collection: moving it writes this component's
					Translation, Rotation and Scale3D.
				</p>
			{/if}
			<label
				class="flex items-center gap-2"
				title="m_bCanBaseOn: off makes its surfaces rooftops pawns slide off; on a wall that is a heavy wall{actor.edit ===
				'component'
					? '. For this mesh it is set on its collection actor, so every mesh of the collection changes.'
					: ''}"
			>
				<input
					type="checkbox"
					class="toggle toggle-xs {actor.canBaseOn ? '' : 'toggle-error'}"
					checked={actor.canBaseOn}
					onchange={(e) => map.setCanBaseOn(e.currentTarget.checked)}
				/>
				<span>Can be stood on</span>
				<span class="font-mono opacity-50"
					>m_bCanBaseOn {actor.canBaseOn ? "True" : "False"}</span
				>
				{#if !actor.canBaseOn}<span class="badge badge-error badge-xs">rooftop</span>{/if}
			</label>
			{#if actor.location}
				<div class="grid grid-cols-[auto_1fr_1fr_1fr] gap-x-2 font-mono">
					<span class="opacity-50">Location</span>
					<span>{fmt(actor.location[0])}</span><span>{fmt(actor.location[1])}</span><span
						>{fmt(actor.location[2])}</span
					>
					{#if actor.rotation}
						<span class="opacity-50">Rotation</span>
						<span>{fmt(actor.rotation[0])}</span><span>{fmt(actor.rotation[1])}</span
						><span>{fmt(actor.rotation[2])}</span>
					{/if}
				</div>
			{/if}
			{#if Object.keys(actor.info).length > 0}
				<div class="flex flex-wrap gap-1">
					{#each Object.entries(actor.info) as [k, v] (k)}
						<span class="badge badge-ghost badge-sm font-mono">{k} {String(v)}</span>
					{/each}
				</div>
			{/if}
			{#if actor.parts.length > 0}
				<div class="flex flex-wrap gap-1">
					{#each actor.parts as p, k (k)}
						<span
							class="badge badge-outline badge-sm font-mono"
							title={p.kind === "mesh" ? p.meshName : p.kind}
							>{p.kind === "mesh"
								? (p.meshName.split(".").at(-1) ?? "mesh")
								: p.kind}</span
						>
					{/each}
				</div>
			{/if}
		</div>
		<CollisionPanel {actor} {level} />
		<div class="min-h-0 flex-1 overflow-auto">
			{#if map.object?.kind === "object"}
				<PropertyTree obj={map.object} path={level} onchange={(u) => map.changed(u)} />
			{:else}
				<div class="flex items-center gap-2 p-3 opacity-60">
					<span class="loading loading-xs loading-spinner"></span> Reading the actor…
				</div>
			{/if}
		</div>
	{/if}
</div>
