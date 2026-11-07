<!-- Bake static light maps for the open map (lightbake.svelte.ts), and show the
     selected mesh's light map as it is stored. -->
<script lang="ts">
	import { untrack } from "svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { lightBake } from "$lib/studio/lightbake.svelte";
	import { map } from "$lib/studio/map.svelte";
	import type { LightMapInfo } from "../../../../electron/src/ipc/contract";

	let info = $state.raw<LightMapInfo | null>(null);
	let infoError = $state<string | null>(null);
	let url = $state<string | null>(null);
	let exposure = $state(1);

	/** The selected actor's first static mesh component. */
	const component = $derived.by(() => {
		const sel = map.selected;
		const actor = map.actor();
		if (!sel || !actor) return null;
		const part = actor.parts.find((p) => p.kind === "mesh");
		return part ? { level: sel.level, index: part.export as number } : null;
	});

	// One entry per level of the map, keeping the ticks already made (read
	// untracked: this effect writes them).
	$effect(() => {
		if (!lightBake.open) return;
		const levels = map.levels;
		const was = untrack(() => lightBake.levels);
		lightBake.levels = Object.fromEntries(levels.map((l) => [l.path, was[l.path] ?? false]));
	});

	$effect(() => {
		const c = component;
		const e = exposure;
		const open = lightBake.open;
		void lightBake.last;
		info = null;
		infoError = null;
		if (!open || !c) return;
		let live = true;
		lightBake
			.info(c.level, c.index, e)
			.then((r) => {
				if (!live) return;
				info = r;
				if (url) URL.revokeObjectURL(url);
				url = r.png
					? URL.createObjectURL(new Blob([r.png as Uint8Array<ArrayBuffer>]))
					: null;
			})
			.catch((error: unknown) => {
				if (live) infoError = error instanceof Error ? error.message : String(error);
			});
		return () => {
			live = false;
		};
	});

	const scopes = [
		{ id: "edited", label: "Edited", detail: "meshes added or moved this session" },
		{ id: "selected", label: "Selected", detail: "the selected mesh or actor" },
		{ id: "level", label: "Levels", detail: "every mesh of the levels ticked (slow)" },
	] as const;

	const fmt = (v: number[] | undefined) => (v ? v.map((x) => x.toFixed(3)).join(" ") : "");
</script>

<Modal bind:open={lightBake.open} title="Bake lighting" width="max-w-xl">
	<div class="space-y-3 text-sm">
		<p class="text-xs opacity-70">
			Direct light from the map's static lights (point, spot, directional, sky) with
			ray-traced shadows against every level, stored like Lightmass stores it (new light map
			textures in the level, the meshes pointed at them). Indirect light is an optional
			one-bounce approximation, so bakes come out darker than Lightmass in shaded areas.
		</p>
		<div class="join">
			{#each scopes as s (s.id)}
				<button
					class="btn join-item btn-sm {lightBake.scope === s.id ? 'btn-active' : ''}"
					onclick={() => (lightBake.scope = s.id)}
					title={s.detail}>{s.label}</button
				>
			{/each}
		</div>
		{#if lightBake.scope === "level"}
			<div class="grid grid-cols-2 gap-x-3">
				{#each map.levels as l (l.path)}
					<label class="flex items-center gap-1.5 text-xs">
						<input
							type="checkbox"
							class="checkbox checkbox-xs"
							bind:checked={lightBake.levels[l.path]}
						/>
						<span class="truncate font-mono">{l.name}</span>
					</label>
				{/each}
			</div>
			<p class="text-warning text-xs">
				A whole level takes from seconds to minutes (Abyss' walls: 187 meshes in about 15 s
				without indirect light; indirect rays multiply it).
			</p>
		{:else if lightBake.scope === "selected"}
			<p class="text-xs opacity-70">
				{map.actor() ? map.actor()?.name : "Nothing selected."}
			</p>
		{/if}
		<div class="grid grid-cols-2 gap-2 text-xs">
			<label class="flex items-center justify-between gap-2"
				>Shadow rays per light
				<input
					type="number"
					min="1"
					max="64"
					class="input input-xs w-20"
					bind:value={lightBake.shadowSamples}
				/></label
			>
			<label class="flex items-center justify-between gap-2"
				>Sky rays
				<input
					type="number"
					min="1"
					max="512"
					class="input input-xs w-20"
					bind:value={lightBake.skySamples}
				/></label
			>
			<label class="flex items-center justify-between gap-2"
				>Indirect rays (0: none)
				<input
					type="number"
					min="0"
					max="256"
					class="input input-xs w-20"
					bind:value={lightBake.indirectRays}
				/></label
			>
			<label class="flex items-center justify-between gap-2"
				>Bounce albedo
				<input
					type="number"
					min="0"
					max="1"
					step="0.05"
					class="input input-xs w-20"
					bind:value={lightBake.albedo}
				/></label
			>
		</div>
		<label class="flex items-center gap-2 text-xs">
			<input type="checkbox" class="checkbox checkbox-xs" bind:checked={lightBake.compare} />
			Compare with the light maps the meshes had
		</label>
		{#if lightBake.last}
			{@const r = lightBake.last}
			<div class="bg-base-200 rounded p-2 text-xs">
				{r.baked.length} baked, {r.skipped.length} skipped, {r.lights} lights, {r.triangles.toLocaleString()}
				shadow triangles, {r.seconds.toFixed(1)} s
				{#if r.comparisons.length}
					{@const m =
						r.comparisons.reduce((a, c) => a + c.stats.meanBaked, 0) /
						r.comparisons.length}
					{@const s =
						r.comparisons.reduce((a, c) => a + c.stats.meanShipped, 0) /
						r.comparisons.length}
					<div>
						Mean irradiance {m.toFixed(3)} baked vs {s.toFixed(3)} before ({r
							.comparisons.length} meshes)
					</div>
				{/if}
				{#each r.skipped.slice(0, 5) as s (s.level + ":" + s.export)}
					<div class="opacity-70">{s.name}: {s.reason}</div>
				{/each}
			</div>
		{/if}
		{#if component}
			<div class="border-base-300 border-t pt-2">
				<div class="mb-1 flex items-center gap-2 text-xs">
					<span class="font-medium">Selected mesh's light map</span>
					<label class="ml-auto flex items-center gap-1 opacity-70"
						>Exposure
						<input
							type="range"
							min="0.25"
							max="8"
							step="0.25"
							class="range range-xs w-24"
							bind:value={exposure}
						/></label
					>
				</div>
				{#if infoError}
					<p class="text-error text-xs">{infoError}</p>
				{:else if info}
					{@const lod = info.lods[0]}
					<div class="flex gap-3">
						{#if url}
							<img
								src={url}
								alt="Irradiance"
								class="border-base-300 h-32 w-32 rounded border object-contain"
								style="image-rendering: pixelated"
							/>
						{/if}
						<div class="min-w-0 text-xs opacity-80">
							{#if !lod || lod.kind === "none"}
								No light map.
							{:else}
								<div>{lod.kind}, {lod.lights?.length ?? 0} lights</div>
								{#each lod.textures ?? [] as t, k (k)}
									<div class="truncate font-mono">
										{t.texture ?? "—"} × {fmt(t.scale)}
									</div>
								{/each}
								<div>
									scale {fmt(lod.coordinateScale)}, bias {fmt(lod.coordinateBias)}
								</div>
								{#if info.decoded}
									<div>
										{info.decoded.width}×{info.decoded.height} texels, mean irradiance
										{fmt(info.decoded.meanIrradiance)}
									</div>
								{:else if info.decodeError}
									<div>{info.decodeError}</div>
								{/if}
							{/if}
						</div>
					</div>
				{/if}
			</div>
		{/if}
		<div class="flex items-center justify-end gap-2">
			{#if lightBake.busy}
				<span class="mr-auto text-xs opacity-70"
					>Baking{lightBake.progress
						? ` ${lightBake.progress.done}/${lightBake.progress.total}`
						: "…"}</span
				>
			{/if}
			<button
				class="btn btn-sm"
				onclick={() => (lightBake.open = false)}
				disabled={lightBake.busy}>Close</button
			>
			<button
				class="btn btn-sm btn-primary"
				onclick={() => lightBake.bake()}
				disabled={lightBake.busy}
			>
				{#if lightBake.busy}<span class="loading loading-xs loading-spinner"></span>{/if}
				Bake
			</button>
		</div>
	</div>
</Modal>
