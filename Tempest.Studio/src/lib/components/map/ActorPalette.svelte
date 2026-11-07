<!-- Add an actor to the chosen level, where the view looks: a copy of one a
	level places (from this map when it has one), a new actor of any
	placeable class built from its defaults, or a static mesh of any package. -->
<script lang="ts">
	import ObjectPicker from "$lib/components/ObjectPicker.svelte";
	import { categoryColor, map } from "$lib/studio/map.svelte";

	type Mode = "copy" | "class" | "mesh";
	const MODES: { id: Mode; label: string; title: string }[] = [
		{
			id: "copy",
			label: "Copy",
			title: "A copy of an actor a level places, with its settings",
		},
		{ id: "class", label: "New", title: "A new actor of a class, with the class defaults" },
		{ id: "mesh", label: "Mesh", title: "A StaticMeshActor showing a mesh of any package" },
	];

	let mode = $state<Mode>("copy");
	let filter = $state("");
	let level = $state(map.targetLevel ?? "");

	$effect(() => {
		if (level) void map.loadActorPalette(level);
	});

	const q = $derived(filter.trim().toLowerCase());
	const copies = $derived(
		(map.actorPalette ?? [])
			.filter((c) => !q || c.class.includes(q) || c.category.includes(q))
			.sort((a, b) => Number(b.inMap) - Number(a.inMap) || b.count - a.count),
	);
	const classes = $derived(
		(map.actorClasses ?? []).filter(
			(c) =>
				!q ||
				c.class.toLowerCase().includes(q) ||
				c.category.includes(q) ||
				c.package.toLowerCase().includes(q),
		),
	);

	function close() {
		map.addingActor = false;
	}

	async function copy(c: { example: { path: string; index: number } }) {
		close();
		await map.addActor(c.example, level);
	}

	async function create(cls: string, mesh?: { path: string; index: number }) {
		close();
		await map.newActor(cls, level, mesh);
	}

	function onEnter() {
		if (mode === "copy" && copies[0]) void copy(copies[0]);
		else if (mode === "class" && classes[0]) void create(classes[0].class);
	}
</script>

<div
	class="bg-base-100 border-base-300 absolute top-1 right-2 z-20 flex w-96 flex-col rounded border text-xs shadow-lg"
	style="max-height:min(460px, 85%)"
	role="dialog"
	tabindex="-1"
	aria-label="Add an actor"
	onpointerdown={(e) => e.stopPropagation()}
	onwheel={(e) => e.stopPropagation()}
	onkeydown={(e) => {
		if (e.key === "Escape") close();
	}}
>
	<div class="flex items-center gap-1.5 p-1.5">
		<div class="join">
			{#each MODES as m (m.id)}
				<button
					class="btn join-item btn-xs {mode === m.id ? 'btn-active' : ''}"
					onclick={() => (mode = m.id)}
					title={m.title}>{m.label}</button
				>
			{/each}
		</div>
		<select class="select select-xs min-w-0 flex-1" bind:value={level} title="Into this level">
			{#each map.levels as l (l.path)}
				<option value={l.path}>{l.name}</option>
			{/each}
		</select>
		<button class="btn btn-ghost btn-xs" onclick={close}>Close</button>
	</div>
	{#if mode !== "mesh"}
		<input
			class="input input-xs mx-1.5 mb-1"
			placeholder={mode === "copy"
				? "Filter (playerstart, jumppad, volume…)"
				: "Filter classes (light, spawn, Engine…)"}
			bind:value={filter}
			onkeydown={(e) => {
				if (e.key === "Enter") onEnter();
			}}
			{@attach (el) => el.focus()}
		/>
	{/if}
	<p class="px-2 pb-1 text-[10px] opacity-60">
		{#if mode === "copy"}
			A copy of an actor a level places, with its settings. From another map its meshes,
			materials and textures come along.
		{:else if mode === "class"}
			A new actor with the class defaults: set what it needs (mesh, target, team…) in the
			inspector. Volumes need a shape: copy one instead.
		{:else}
			A new StaticMeshActor showing the mesh; a mesh from another package is copied in with
			its materials and textures. It has no baked lighting.
		{/if}
		Placed where the view looks; navigation paths are not rebuilt.
	</p>
	<div class="min-h-0 flex-1 overflow-auto pb-1">
		{#if !map.actorPalette}
			<div class="flex items-center gap-2 p-2 opacity-60">
				<span class="loading loading-xs loading-spinner"></span> Listing the actors…
			</div>
		{:else if mode === "copy"}
			{#each copies.slice(0, 300) as c (c.class)}
				<button
					class="hover:bg-base-300 flex w-full items-center gap-2 px-2 py-0.5 text-left"
					onclick={() => copy(c)}
					title="Copied from {c.example.path.split(/[\\/]/).at(-1)} (export {c.example
						.index})"
				>
					<span
						class="h-2 w-2 shrink-0 rounded-full"
						style="background:{categoryColor(c.category)}"
					></span>
					<span class="flex-1 truncate font-mono">{c.class}</span>
					{#if !c.inMap}<span
							class="badge badge-ghost badge-xs"
							title="No example in this map">other map</span
						>{/if}
					<span class="opacity-40">{c.count}</span>
				</button>
			{/each}
		{:else if mode === "class"}
			{#each classes.slice(0, 400) as c (c.class)}
				<button
					class="hover:bg-base-300 flex w-full items-center gap-2 px-2 py-0.5 text-left"
					onclick={() => create(c.class)}
					title="{c.class} ({c.package})"
				>
					<span
						class="h-2 w-2 shrink-0 rounded-full"
						style="background:{categoryColor(c.category)}"
					></span>
					<span class="flex-1 truncate font-mono">{c.class}</span>
					<span class="opacity-40">{c.package}</span>
				</button>
			{/each}
		{:else}
			<div class="px-1.5">
				<ObjectPicker
					isA="StaticMesh"
					placeholder="type part of a mesh's name or path"
					onpick={(hit) => create("StaticMeshActor", hit)}
				/>
			</div>
		{/if}
	</div>
</div>
