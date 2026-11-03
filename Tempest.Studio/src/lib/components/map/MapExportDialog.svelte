<!-- Export the open map (the levels and kinds shown, by default) as one GLB or FBX. -->
<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { pickSave } from "$lib/platform/api";
	import { basename, bytes } from "$lib/studio/format";
	import { CATEGORIES, map } from "$lib/studio/map.svelte";
	import { onUpkEvent, rpc } from "$lib/studio/rpc";
	import { toasts } from "$lib/studio/toast.svelte";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let format = $state<"glb" | "fbx">("glb");
	let levels = $state<Record<string, boolean>>({});
	let skip = $state<Record<string, boolean>>({});
	let volumes = $state(false);
	let textures = $state(true);
	let textureSize = $state(1024);
	let busy = $state(false);
	let progress = $state<{ done: number; total: number } | null>(null);

	$effect(() => {
		if (!open) return;
		// Start from what the view shows.
		levels = Object.fromEntries(map.levels.map((l) => [l.path, !map.hiddenLevels[l.path]]));
		skip = Object.fromEntries(
			CATEGORIES.map((c) => [c.id, Boolean(map.hiddenCategories[c.id])]),
		);
	});

	async function run() {
		if (!map.path) return;
		const name = basename(map.path).replace(/\.upk$/i, "");
		const out = await pickSave(
			"Export the map",
			`${name}.${format}`,
			format === "glb"
				? [{ name: "glTF binary", extensions: ["glb"] }]
				: [{ name: "FBX", extensions: ["fbx"] }],
		);
		if (!out) return;
		busy = true;
		progress = null;
		const stop = onUpkEvent((e) => {
			if (e.event === "map.progress") {
				progress = { done: Number(e.done), total: Number(e.total) };
			}
		});
		try {
			const r = await rpc("map.export", {
				path: map.path,
				out,
				levels: Object.entries(levels)
					.filter(([, on]) => on)
					.map(([p]) => p),
				skip: Object.entries(skip)
					.filter(([, on]) => on)
					.map(([c]) => c),
				volumes,
				textures,
				textureSize,
			});
			toasts.push(
				r.warningCount > 0 ? "warning" : "success",
				`${basename(r.out)}: ${r.placements} placements of ${r.meshes} meshes, ${bytes(r.bytes)}${r.warningCount > 0 ? `; ${r.warningCount} warnings: ${r.warnings.slice(0, 3).join("; ")}` : ""}`,
				"Map exported",
			);
			open = false;
		} catch (error) {
			toasts.error(error, "Could not export the map");
		} finally {
			stop();
			busy = false;
		}
	}
</script>

<Modal bind:open title="Export the map" width="max-w-lg">
	<div class="space-y-3 text-sm">
		<p class="text-xs opacity-70">
			One file with every placed mesh, its materials and textures. Each mesh is stored once
			and placed by a node per placement, grouped by level; positions in metres (GLB) or
			centimetres (FBX), Y up.
		</p>
		<div class="join">
			<button
				class="btn join-item btn-sm {format === 'glb' ? 'btn-active' : ''}"
				onclick={() => (format = "glb")}>GLB (glTF)</button
			>
			<button
				class="btn join-item btn-sm {format === 'fbx' ? 'btn-active' : ''}"
				onclick={() => (format = "fbx")}>FBX</button
			>
		</div>
		<div>
			<div class="mb-1 text-xs opacity-70">Levels</div>
			<div class="grid grid-cols-2 gap-x-3">
				{#each map.levels as l (l.path)}
					<label class="flex items-center gap-1.5 text-xs">
						<input
							type="checkbox"
							class="checkbox checkbox-xs"
							bind:checked={levels[l.path]}
						/>
						<span class="truncate font-mono">{l.name}</span>
					</label>
				{/each}
			</div>
		</div>
		<div>
			<div class="mb-1 text-xs opacity-70">Leave out</div>
			<div class="grid grid-cols-3 gap-x-3">
				{#each CATEGORIES as c (c.id)}
					<label class="flex items-center gap-1.5 text-xs">
						<input
							type="checkbox"
							class="checkbox checkbox-xs"
							bind:checked={skip[c.id]}
						/>
						<span class="truncate">{c.label}</span>
					</label>
				{/each}
			</div>
		</div>
		<label class="flex items-center gap-2 text-xs">
			<input type="checkbox" class="checkbox checkbox-xs" bind:checked={volumes} />
			Volumes as meshes (blocking, triggers… of the kinds kept)
		</label>
		<div class="flex items-center gap-2 text-xs">
			<label class="flex items-center gap-2">
				<input type="checkbox" class="checkbox checkbox-xs" bind:checked={textures} />
				Textures
			</label>
			<select class="select select-xs w-36" bind:value={textureSize} disabled={!textures}>
				<option value={512}>up to 512²</option>
				<option value={1024}>up to 1024²</option>
				<option value={2048}>up to 2048²</option>
				<option value={8192}>full size</option>
			</select>
		</div>
		<div class="flex items-center justify-end gap-2">
			{#if busy}
				<span class="mr-auto text-xs opacity-70"
					>Exporting{progress
						? ` ${Math.round((progress.done / progress.total) * 100)}%`
						: "…"}</span
				>
			{/if}
			<button class="btn btn-sm" onclick={() => (open = false)} disabled={busy}>Cancel</button
			>
			<button class="btn btn-sm btn-primary" onclick={run} disabled={busy}>
				{#if busy}<span class="loading loading-xs loading-spinner"></span>{/if}
				Export…
			</button>
		</div>
	</div>
</Modal>
