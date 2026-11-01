<!--
	The material slots of a mesh (`mesh.materials`): what each slot draws
	with, opened with a click to edit its textures and parameters. Change…
	points a slot at any material or instance of the game folder; "Make it
	this mesh's own" copies a shared instance next to the mesh, so editing
	it changes this mesh only.
-->
<script lang="ts">
	import { Copy, Replace } from "@lucide/svelte";
	import ObjectPicker from "$lib/components/ObjectPicker.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { MeshMaterials, ObjRef } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
		/** After a slot changed: redraw the mesh. */
		onchanged?: () => void;
	}
	const { path, index, onchanged }: Props = $props();

	let info = $state<MeshMaterials | null>(null);
	let picking = $state<number | null>(null);
	let busy = $state(false);

	const MATERIALS = ["material", "materialinstanceconstant", "materialinstancetimevarying"];

	async function load() {
		try {
			info = await rpc("mesh.materials", { path, index });
		} catch {
			info = null;
		}
	}

	$effect(() => {
		void path;
		void index;
		void studio.edits;
		void load();
	});

	async function changed(next: MeshMaterials, what: string) {
		info = next;
		picking = null;
		studio.markDirty(path);
		await studio.reloadTables();
		onchanged?.();
		toasts.push("success", what, "Material");
	}

	async function set(slot: number, target: { path: string; index: number }) {
		busy = true;
		try {
			const r = await rpc("mesh.setMaterial", { path, index, slot, material: target });
			await changed(
				r,
				`Slot ${slot} now draws with ${r.slots.find((s) => s.slot === slot)?.material.path}`,
			);
		} catch (error) {
			toasts.error(error, "Could not change the material");
		} finally {
			busy = false;
		}
	}

	async function own(slot: number) {
		busy = true;
		try {
			const r = await rpc("mesh.ownMaterial", { path, index, slot });
			await changed(
				r,
				`${r.material.path} is this mesh's own: open it to change its textures and colours`,
			);
		} catch (error) {
			toasts.error(error, "Could not copy the material");
		} finally {
			busy = false;
		}
	}

	async function open(m: ObjRef) {
		if (m.ref > 0) await studio.select(m.ref - 1);
		else if (m.ref < 0) await studio.openObjectPath(m.path);
	}
</script>

{#if info && info.slots.length > 0}
	<div class="flex flex-col gap-1 text-[11px]">
		<span class="font-sans font-semibold opacity-70">Materials</span>
		{#each info.slots as s (s.slot)}
			<div class="flex flex-wrap items-center gap-2">
				<span class="w-10 shrink-0 opacity-50">slot {s.slot}</span>
				{#if s.material.ref === 0}
					<span class="opacity-50"
						>none (map props get theirs from where they are placed)</span
					>
				{:else}
					<button
						class="link link-hover min-w-0 truncate"
						onclick={() => open(s.material)}
						title="Open it to change its textures, colours and parameters"
						>{s.material.path}</button
					>
					<span class="badge badge-ghost badge-xs"
						>{s.instance ? "instance" : "base material"}</span
					>
					{#if s.external}
						<span
							class="badge badge-ghost badge-xs"
							title="Shared with everything else using it; edits there change all of them"
							>other package</span
						>
					{/if}
				{/if}
				<span class="flex-1"></span>
				<button
					class="btn btn-ghost btn-xs"
					disabled={busy}
					onclick={() => (picking = picking === s.slot ? null : s.slot)}
					><Replace size={11} /> Change…</button
				>
				{#if s.instance}
					<button
						class="btn btn-ghost btn-xs"
						disabled={busy}
						onclick={() => own(s.slot)}
						title="Copy this instance next to the mesh and use the copy: editing it changes this mesh only"
						><Copy size={11} /> Make it this mesh's own</button
					>
				{/if}
			</div>
			{#if picking === s.slot}
				<div class="bg-base-200 rounded p-2">
					<ObjectPicker
						isA="MaterialInterface"
						local={MATERIALS}
						placeholder="a material or instance, e.g. mic_ or the champion's name"
						onpick={(h) => set(s.slot, { path: h.path, index: h.index })}
					/>
				</div>
			{/if}
		{/each}
	</div>
{/if}
