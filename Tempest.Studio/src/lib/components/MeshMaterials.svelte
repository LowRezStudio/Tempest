<!--
	The material slots of a mesh (`mesh.materials`): what each slot draws
	with, opened with a click to edit its textures and parameters. Change…
	points a slot at any material or instance of the game folder; "Make it
	this mesh's own" gives the slot a material of its own next to the mesh
	(a copy of a shared instance, or a new instance of a base material), so
	editing it changes this mesh only; "Own materials for every slot" does
	that for all slots at once, in one group named after the mesh. Skeletal meshes can get a slot more, or lose
	one no section draws with (the per-section and per-slot tables follow).
-->
<script lang="ts">
	import { Copy, Plus, Replace, Trash2 } from "@lucide/svelte";
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
			toasts.error(error, "Could not make the material");
		} finally {
			busy = false;
		}
	}

	async function ownAll() {
		busy = true;
		try {
			const r = await rpc("mesh.ownMaterials", { path, index });
			const skipped = r.skipped.filter((x) => x.reason !== "already the mesh's own");
			await changed(
				r,
				r.made.length > 0
					? `${r.made.length} material${r.made.length === 1 ? "" : "s"} of this mesh's own in ${r.group.path}: open one to change its textures`
					: "Every slot already has the mesh's own material",
			);
			for (const x of skipped)
				toasts.push("warning", `Slot ${x.slot}: ${x.reason}`, "Material");
		} catch (error) {
			toasts.error(error, "Could not make the materials");
		} finally {
			busy = false;
		}
	}

	async function addSlot() {
		busy = true;
		try {
			const r = await rpc("mesh.addSlot", { path, index });
			await changed(
				r,
				`Slot ${r.slots.length - 1} added: no section draws with it until a mesh using it is imported`,
			);
		} catch (error) {
			toasts.error(error, "Could not add a slot");
		} finally {
			busy = false;
		}
	}

	async function removeSlot(slot: number) {
		busy = true;
		try {
			const r = await rpc("mesh.removeSlot", { path, index, slot });
			await changed(r, `Slot ${slot} removed`);
		} catch (error) {
			toasts.error(error, "Could not remove the slot");
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
		<div class="flex items-center gap-2">
			<span class="font-sans font-semibold opacity-70">Materials</span>
			<span class="flex-1"></span>
			<button
				class="btn btn-ghost btn-xs"
				disabled={busy}
				onclick={ownAll}
				title="Give every slot a material of this mesh's own, gathered in one group named after the mesh: then change their textures without touching other meshes"
				><Copy size={11} /> Own materials for every slot</button
			>
			{#if info.skeletal}
				<button
					class="btn btn-ghost btn-xs"
					disabled={busy}
					onclick={addSlot}
					title="A new slot drawn with the last slot's material; sections use it once a mesh with that many materials is imported"
					><Plus size={11} /> Add slot</button
				>
			{/if}
		</div>
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
				{#if s.material.ref !== 0}
					<button
						class="btn btn-ghost btn-xs"
						disabled={busy}
						onclick={() => own(s.slot)}
						title={s.instance
							? "Copy this instance next to the mesh and use the copy: editing it changes this mesh only"
							: "Make a new instance of this base material next to the mesh and use it: set its textures and colours for this mesh only"}
						><Copy size={11} /> Make it this mesh's own</button
					>
				{/if}
				{#if info.skeletal && info.slots.length > 1}
					<button
						class="btn btn-ghost btn-xs"
						disabled={busy || s.sections > 0}
						onclick={() => removeSlot(s.slot)}
						title={s.sections > 0
							? `${s.sections} section${s.sections === 1 ? "" : "s"} draw with it: import a mesh that does not use it first`
							: "Remove this slot (later slots move down)"}
						><Trash2 size={11} /></button
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
