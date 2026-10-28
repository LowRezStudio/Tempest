<!--
	Export an animation (or a whole AnimSet) with its mesh and skeleton as glTF
	or FBX: one sequence, or every sequence of the AnimSet as one animation each,
	named by its SequenceName (what Replace… matches them back by).
-->
<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { pickSave } from "$lib/platform/api";
	import { bytes, safeName } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { AnimInfo, ExportRow } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false), row }: { open?: boolean; row: ExportRow } = $props();

	let info = $state<AnimInfo | null>(null);
	let all = $state(false);
	let format = $state<"glb" | "fbx">("glb");
	let working = $state(false);
	const isSet = $derived(row.class.toLowerCase() === "animset");

	$effect(() => {
		if (!open || !studio.pkg) return;
		info = null;
		all = isSet;
		const path = studio.pkg.path;
		void rpc("anim.info", { path, index: row.i })
			.then((i) => (info = i))
			.catch((error: unknown) => toasts.error(error, "Export"));
	});

	async function submit(event: Event) {
		event.preventDefault();
		if (!studio.pkg) return;
		const path = studio.pkg.path;
		const setName = info?.animSet.path ?? row.path;
		const out = await pickSave(
			"Export animation",
			`${safeName(all ? setName : row.path)}.${format}`,
			[
				format === "glb"
					? { name: "glTF binary (mesh, skeleton, animations)", extensions: ["glb"] }
					: { name: "Autodesk FBX (mesh, skeleton, animations)", extensions: ["fbx"] },
			],
		);
		if (!out) return;
		working = true;
		try {
			const r = await studio.run("Exporting animation…", () =>
				rpc("anim.export", {
					path,
					index: isSet ? (info?.index ?? row.i) : row.i,
					out,
					all,
				}),
			);
			for (const w of r.warnings) toasts.push("warning", w, "Texture skipped");
			toasts.push(
				"success",
				`${r.animations} animation${r.animations === 1 ? "" : "s"} on ${r.mesh.name}, ${bytes(r.bytes ?? 0)} → ${out}`,
				"Exported",
			);
			open = false;
		} catch (error) {
			toasts.error(error, "Export failed");
		} finally {
			working = false;
		}
	}
</script>

<Modal bind:open title="Export {row.name}" width="max-w-md">
	<form onsubmit={submit} class="space-y-3 text-sm">
		<div class="flex gap-4">
			<label class="flex items-center gap-2"
				><input type="radio" class="radio radio-sm" value="glb" bind:group={format} /> glTF (.glb)</label
			>
			<label class="flex items-center gap-2"
				><input type="radio" class="radio radio-sm" value="fbx" bind:group={format} /> FBX</label
			>
		</div>
		<label class="flex items-start gap-2">
			<input type="checkbox" class="checkbox checkbox-sm mt-0.5" bind:checked={all} />
			<span>
				Every sequence of the AnimSet{info ? ` (${info.sequences.length})` : ""}
				<span class="block text-xs opacity-60"
					>One animation per sequence, named like it ({info?.sequences
						.slice(0, 3)
						.map((q) => q.name)
						.join(", ") ?? "…"}{(info?.sequences.length ?? 0) > 3 ? ", …" : ""}). Keep
					the names to replace them all at once later.</span
				>
			</span>
		</label>
		<p class="text-xs opacity-60">
			The file holds the mesh{info?.meshes[0]
				? ` (${info.meshes[0].object.split(".").at(-1)})`
				: ""}, its skeleton and the animation{all ? "s" : ""}, sampled at every frame.
		</p>
		<div class="flex justify-end gap-2">
			<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}
				>Cancel</button
			>
			<button class="btn btn-sm btn-primary" type="submit" disabled={!info || working}
				>Export…</button
			>
		</div>
	</form>
</Modal>
