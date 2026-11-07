<!-- The selected actor's collision (inspector): its switches as toggles and,
	for meshes, what pawns and shots test; for volumes a Shape panel that
	rebuilds the brush with an UnrealEd builder (box, cylinder, cone, sphere),
	from a mesh's hull or bounds, or from edited vertices
	(`collision.svelte.ts`, `map.setBrush`). -->
<script lang="ts">
	import {
		BLOCK_COLORS,
		BLOCK_LABELS,
		BRUSH_DEFAULTS,
		collision,
		type BrushInfo,
	} from "$lib/studio/collision.svelte";
	import type { BrushSpec, MapActor } from "../../../../electron/src/ipc/contract";

	let { actor, level }: { actor: MapActor; level: string } = $props();

	const isVolume = $derived(actor.parts.some((p) => p.kind === "brush"));
	const items = $derived(collision.data ? collision.itemsOf(level, actor.export) : []);

	type Kind = keyof typeof BRUSH_DEFAULTS | "mesh" | "vertices";
	let kind = $state<Kind>("box");
	let spec = $state<Record<string, string | number | boolean>>({ ...BRUSH_DEFAULTS.box });
	let meshObject = $state("");
	let meshFit = $state<"hull" | "bounds">("hull");
	let info = $state.raw<BrushInfo | null>(null);
	let vertices = $state<[number, number, number][]>([]);
	let busy = $state(false);

	const FIELDS: Record<string, { key: string; label: string; int?: boolean }[]> = {
		box: [
			{ key: "x", label: "X" },
			{ key: "y", label: "Y" },
			{ key: "z", label: "Z" },
			{ key: "wallThickness", label: "Wall" },
		],
		cylinder: [
			{ key: "z", label: "Height" },
			{ key: "outerRadius", label: "Radius" },
			{ key: "innerRadius", label: "Inner radius" },
			{ key: "sides", label: "Sides", int: true },
		],
		cone: [
			{ key: "z", label: "Height" },
			{ key: "capZ", label: "Cap height" },
			{ key: "outerRadius", label: "Radius" },
			{ key: "innerRadius", label: "Inner radius" },
			{ key: "sides", label: "Sides", int: true },
		],
		sphere: [
			{ key: "radius", label: "Radius" },
			{ key: "sphereExtrapolation", label: "Subdivisions (1–5)", int: true },
		],
	};

	function pick(k: Kind) {
		kind = k;
		if (k in BRUSH_DEFAULTS) spec = { ...BRUSH_DEFAULTS[k as keyof typeof BRUSH_DEFAULTS] };
	}

	async function readShape() {
		info = await collision.brush(level, actor.export);
		const seen = new Set<string>();
		vertices = [];
		for (const poly of info?.polys ?? []) {
			for (const v of poly) {
				const k = v.map((x) => x.toFixed(2)).join(",");
				if (!seen.has(k)) {
					seen.add(k);
					vertices.push([v[0], v[1], v[2]]);
				}
			}
		}
	}

	$effect(() => {
		void [actor.export, level];
		info = null;
		vertices = [];
	});

	async function apply() {
		let brush: BrushSpec;
		if (kind === "mesh") {
			brush = { kind: "mesh", mesh: { object: meshObject.trim() }, fit: meshFit };
		} else if (kind === "vertices") brush = { kind: "hull", points: $state.snapshot(vertices) };
		else brush = { ...(spec as object), kind } as BrushSpec;
		busy = true;
		if (await collision.setBrush(level, actor.export, brush)) await readShape();
		busy = false;
	}

	const FLAG_HELP: Record<string, string> = {
		bCollideActors: "Actor: takes part in collision at all",
		bBlockActors: "Actor: blocks other actors' movement",
		CollideActors: "Component: takes part in collision",
		BlockActors: "Component: blocks actors (pawns)",
		BlockNonZeroExtent: "Component: blocks box sweeps (pawn movement)",
		BlockZeroExtent: "Component: blocks line traces (shots, hitscan)",
		BlockRigidBody: "Component: blocks physics bodies",
	};
</script>

<div class="border-base-300 space-y-2 border-b p-2">
	<div class="flex items-center gap-2">
		<span class="font-semibold">Collision</span>
		{#if !collision.data}
			<button
				class="btn btn-ghost btn-xs"
				onclick={() => void collision.load()}
				disabled={collision.loading}>Read collision</button
			>
		{/if}
	</div>
	{#each items as it (it.kind + it.export)}
		<div class="space-y-1">
			<div class="flex items-center gap-2">
				<span
					class="h-2.5 w-2.5 rounded-sm"
					style="background:{BLOCK_COLORS[it.flags.blocks]}"
				></span>
				<span class="truncate font-mono">{it.kind === "mesh" ? it.name : actor.name}</span>
				<span class="opacity-60">{BLOCK_LABELS[it.flags.blocks]}</span>
			</div>
			{#if it.kind === "mesh"}
				{@const mc = collision.data?.meshes[it.meshKey]}
				{#if mc}
					<div class="flex flex-wrap gap-1 opacity-80">
						<span
							class="badge badge-ghost badge-sm"
							title="What pawn sweeps test (UseSimpleBoxCollision)"
							>pawns: {mc.pawns}</span
						>
						<span
							class="badge badge-ghost badge-sm"
							title="What traces test (UseSimpleLineCollision)"
							>shots: {mc.shots}</span
						>
						{#if !mc.bodySetup}<span
								class="badge badge-warning badge-sm"
								title="Simple collision is asked for but the mesh has no body setup: nothing is hit"
								>no body setup</span
							>{/if}
					</div>
				{/if}
			{/if}
			<div class="grid grid-cols-2 gap-x-2">
				{#each Object.entries(FLAG_HELP) as [f, help] (f)}
					{#if typeof it.flags[f] === "boolean"}
						<label class="flex items-center gap-1" title={help}>
							<input
								type="checkbox"
								class="toggle toggle-xs"
								checked={it.flags[f] as boolean}
								onchange={(e) =>
									void collision.setFlag(
										level,
										f.startsWith("b")
											? it.kind === "mesh"
												? it.parent
												: it.export
											: it.kind === "mesh"
												? it.export
												: it.component,
										f,
										e.currentTarget.checked,
									)}
							/>
							<span class="font-mono">{f}</span>
						</label>
					{/if}
				{/each}
			</div>
		</div>
	{/each}

	{#if isVolume}
		<div class="space-y-1">
			<div class="flex items-center gap-2">
				<span class="font-semibold">Shape</span>
				<select
					class="select select-xs w-28"
					value={kind}
					onchange={(e) => pick(e.currentTarget.value as Kind)}
				>
					<option value="box">Box</option>
					<option value="cylinder">Cylinder</option>
					<option value="cone">Cone</option>
					<option value="sphere">Sphere</option>
					<option value="mesh">From a mesh</option>
					<option value="vertices">Edit vertices</option>
				</select>
				<button
					class="btn btn-ghost btn-xs"
					onclick={() => void readShape()}
					title="Read the current brush">Current</button
				>
				{#if info}<span class="opacity-60"
						>{info.polys.length} polys, {info.hulls.length} hull{info.hulls.length === 1
							? ""
							: "s"}{info.fromBrush ? "" : " (cooked: no brush)"}</span
					>{/if}
			</div>
			{#if kind in FIELDS}
				<div class="grid grid-cols-2 gap-1">
					{#each FIELDS[kind] as f (f.key)}
						<label class="flex items-center gap-1">
							<span class="w-20 opacity-60">{f.label}</span>
							<input
								type="number"
								class="input input-xs w-full font-mono"
								step={f.int ? 1 : 16}
								value={spec[f.key] as number}
								onchange={(e) =>
									(spec[f.key] = f.int
										? Math.round(e.currentTarget.valueAsNumber)
										: e.currentTarget.valueAsNumber)}
							/>
						</label>
					{/each}
				</div>
				<div class="flex flex-wrap gap-3">
					{#if kind !== "sphere"}
						<label class="flex items-center gap-1"
							><input
								type="checkbox"
								class="checkbox checkbox-xs"
								bind:checked={spec.hollow as boolean}
							/> Hollow</label
						>
					{/if}
					{#if kind === "cylinder" || kind === "cone"}
						<label class="flex items-center gap-1"
							><input
								type="checkbox"
								class="checkbox checkbox-xs"
								bind:checked={spec.alignToSide as boolean}
							/> Align to side</label
						>
					{/if}
				</div>
			{:else if kind === "mesh"}
				<input
					class="input input-xs w-full font-mono"
					placeholder="Static mesh object path"
					bind:value={meshObject}
				/>
				<label class="flex items-center gap-1"
					><input
						type="checkbox"
						class="checkbox checkbox-xs"
						checked={meshFit === "bounds"}
						onchange={(e) => (meshFit = e.currentTarget.checked ? "bounds" : "hull")}
					/> Bounding box instead of the convex hull</label
				>
			{:else}
				<p class="opacity-60">
					The volume becomes the convex hull of these points (actor space, Unreal units).
					Read the current shape first.
				</p>
				<div class="max-h-48 overflow-auto">
					{#each vertices as v, k (k)}
						<div class="grid grid-cols-3 gap-1">
							{#each [0, 1, 2] as c (c)}
								<input
									type="number"
									class="input input-xs font-mono"
									value={Math.round(v[c] * 100) / 100}
									onchange={(e) =>
										(vertices[k][c] = e.currentTarget.valueAsNumber)}
								/>
							{/each}
						</div>
					{/each}
				</div>
			{/if}
			<button
				class="btn btn-primary btn-xs"
				disabled={busy ||
					(kind === "vertices" && vertices.length < 4) ||
					(kind === "mesh" && !meshObject.trim())}
				onclick={() => void apply()}>Apply shape</button
			>
		</div>
	{/if}
</div>
