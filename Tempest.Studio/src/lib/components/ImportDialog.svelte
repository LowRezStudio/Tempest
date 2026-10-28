<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { pickFile } from "$lib/platform/api";
	import { bytes } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { Preview } from "$lib/studio/classes";
	import type { ExportRow } from "../../../electron/src/ipc/contract";

	let {
		open = $bindable(false),
		row,
		kind,
	}: { open?: boolean; row: ExportRow; kind: Preview } = $props();

	type Mode = "texture" | "mesh" | "sound" | "swf" | "anim" | "raw";
	let mode = $state<Mode>("raw");
	let file = $state("");
	let mips = $state<"auto" | "yes" | "no">("auto");
	let format = $state("");
	let lod = $state(0);
	let working = $state(false);
	/** Animations in the chosen file (anim mode), and which one to import. */
	let animations = $state<string[]>([]);
	let animation = $state(0);
	/** Every animation of the file into the AnimSet's sequence of the same name. */
	let allSequences = $state(false);
	/** The AnimSet's sequences (anim mode), to show what the file replaces. */
	let sequences = $state<{ index: number; name: string }[]>([]);
	const isSet = $derived(row.class.toLowerCase() === "animset");
	/** How `upk` pairs them: the name after an `Armature|` prefix, any case. */
	const leaf = (n: string) => (n.split("|").at(-1) ?? n).trim().toLowerCase();
	const pairs = $derived.by(() => {
		const free = new Set(sequences.map((q) => q.name.toLowerCase()));
		return animations.map((a) => {
			const l = leaf(a);
			const hit = free.has(l);
			if (hit) free.delete(l);
			return { animation: a, sequence: hit ? l : null };
		});
	});

	const FORMATS = ["", "PF_DXT1", "PF_DXT5", "PF_A8R8G8B8", "PF_G8"];

	$effect(() => {
		if (open) {
			file = "";
			allSequences = row.class.toLowerCase() === "animset";
			sequences = [];
			if (kind === "anim" && studio.pkg) {
				void rpc("anim.info", { path: studio.pkg.path, index: row.i })
					.then((i) => (sequences = i.sequences))
					.catch(() => {});
			}
			mode =
				kind === "none" || kind === "material" || kind === "wwise" || kind === "particle"
					? "raw"
					: (kind as Mode);
		}
	});

	const filters = $derived.by(() => {
		switch (mode) {
			case "texture": {
				return [{ name: "Images", extensions: ["png", "dds"] }];
			}
			case "mesh": {
				return [{ name: "Meshes", extensions: ["glb", "gltf", "fbx", "obj"] }];
			}
			case "sound": {
				return [
					{
						name: "Audio (Ogg Vorbis, WAV, Wwise .wem)",
						extensions: ["ogg", "wav", "wem"],
					},
				];
			}
			case "swf": {
				return [{ name: "Scaleform / Flash", extensions: ["gfx", "swf"] }];
			}
			case "anim": {
				return [{ name: "Animations (glTF, FBX)", extensions: ["glb", "gltf", "fbx"] }];
			}
			default: {
				return [{ name: "Raw payload", extensions: ["bin", "*"] }];
			}
		}
	});

	async function browse() {
		const f = await pickFile("Replace with", filters);
		if (f) file = f;
	}

	// The animations of the chosen file (a file can hold several).
	$effect(() => {
		const f = file;
		animations = [];
		animation = 0;
		if (mode !== "anim" || !f) return;
		void rpc("anim.files", { file: f })
			.then((r) => {
				if (file === f) animations = r.animations;
			})
			.catch(() => {});
	});

	async function submit(event: Event) {
		event.preventDefault();
		if (!studio.pkg || !file) return;
		working = true;
		const path = studio.pkg.path;
		try {
			if (mode === "texture") {
				const info = await studio.run("Encoding texture…", () =>
					rpc("texture.import", {
						path,
						index: row.i,
						file,
						mips: mips === "auto" ? undefined : mips === "yes",
						format: format || undefined,
					}),
				);
				toasts.push(
					"success",
					`${info.width}×${info.height} ${info.format}, ${info.mips.length} mips stored inline`,
					"Texture replaced",
				);
			} else if (mode === "mesh") {
				const r = await studio.run("Building mesh…", () =>
					rpc("mesh.import", { path, index: row.i, file, lod }),
				);
				for (const w of r.warnings) toasts.push("warning", w, "Mesh import");
				toasts.push(
					"success",
					`${r.vertices} vertices, ${r.triangles} triangles, ${r.sections} sections${r.chunks ? `, ${r.chunks} chunks` : ""}`,
					"Mesh replaced",
				);
			} else if (mode === "anim") {
				const all = isSet || allSequences;
				const r = await studio.run("Importing animation…", () =>
					rpc("anim.import", { path, index: row.i, file, animation, all }),
				);
				const kept = r.bonesMissing.length
					? `; ${r.bonesMissing.length} bones not in the file keep their pose`
					: "";
				if (all) {
					const skipped = r.unmatched.length
						? `; not matched: ${r.unmatched.join(", ")}`
						: "";
					toasts.push(
						"success",
						`${r.replaced.length} sequence${r.replaced.length === 1 ? "" : "s"} replaced (${r.replaced.map((x) => x.sequence).join(", ")})${skipped}${kept}`,
						"Animations replaced",
					);
				} else {
					toasts.push(
						"success",
						`${r.animation}: ${r.numFrames} frames, ${r.length.toFixed(2)} s, ${r.bonesMatched} bones${kept}`,
						"Animation replaced",
					);
				}
			} else if (mode === "sound" || mode === "swf") {
				const r = await rpc("media.import", { path, index: row.i, file });
				toasts.push("success", `${bytes(r.bytes)} of ${r.kind} data`, "Replaced");
			} else {
				const r = await rpc("object.replaceRaw", { path, index: row.i, file });
				if (r.error) {
					toasts.push(
						"warning",
						`${bytes(r.bytes)} written, but the payload does not parse as a ${row.class}: ${r.error}`,
						"Raw payload replaced",
					);
				} else {
					toasts.push(
						"success",
						`${bytes(r.bytes)} written (${r.kind})`,
						"Raw payload replaced",
					);
				}
			}
			studio.markDirty();
			await studio.select(row.i);
			open = false;
		} catch (error) {
			toasts.error(error, "Replace failed");
		} finally {
			working = false;
		}
	}
</script>

<Modal bind:open title="Replace {row.name}" width="max-w-lg">
	<form onsubmit={submit} class="space-y-3 text-sm">
		{#if kind !== "none" && kind !== "material" && kind !== "wwise" && kind !== "particle"}
			<div role="tablist" class="tabs-box tabs tabs-xs">
				<button
					role="tab"
					type="button"
					class="tab {mode !== 'raw' ? 'tab-active' : ''}"
					onclick={() => (mode = kind)}
				>
					{kind === "texture"
						? "Image"
						: kind === "anim"
							? "Animation file"
							: kind === "mesh"
								? "Mesh file"
								: kind === "sound"
									? "Audio"
									: "Movie"}
				</button>
				<button
					role="tab"
					type="button"
					class="tab {mode === 'raw' ? 'tab-active' : ''}"
					onclick={() => (mode = "raw")}
				>
					Raw payload
				</button>
			</div>
		{/if}
		<div class="flex gap-1">
			<input
				class="input input-sm flex-1 font-mono text-xs"
				bind:value={file}
				placeholder="file to import"
			/>
			<button class="btn btn-sm" type="button" onclick={browse}>Browse…</button>
		</div>
		{#if mode === "anim"}
			<label class="flex items-start gap-2">
				<input
					type="checkbox"
					class="checkbox checkbox-sm mt-0.5"
					bind:checked={allSequences}
					disabled={isSet}
				/>
				<span>
					Every sequence of the AnimSet{sequences.length ? ` (${sequences.length})` : ""}
					<span class="block text-xs opacity-60"
						>Each animation of the file replaces the sequence with its name (as Export
						names them); the others are left as they are.</span
					>
				</span>
			</label>
			{#if (isSet || allSequences) && animations.length > 0}
				<div class="border-base-300 max-h-40 overflow-auto rounded border p-2 text-xs">
					{#each pairs as p, k (k)}
						<div class="flex gap-2 {p.sequence ? '' : 'opacity-50'}">
							<span class="font-mono">{p.animation}</span>
							<span class="opacity-60">→</span>
							<span>{p.sequence ?? "no sequence of that name, skipped"}</span>
						</div>
					{/each}
				</div>
			{:else if animations.length > 1}
				<label class="block">
					<span class="text-xs opacity-70">Animation in the file</span>
					<select class="select select-sm mt-1 w-full" bind:value={animation}>
						{#each animations as a, k (k)}<option value={k}>{a}</option>{/each}
					</select>
				</label>
			{/if}
			<p class="text-xs opacity-60">
				Bones are matched by name with the animation's mesh; the file's bones can point any
				way (Blender re-orients them), only their names and bind pose matter. The motion is
				sampled at the sequence's frame rate, keyed like the game's own sequences, and
				notify times follow the new length. In Blender, set the scene to 30 fps before
				importing a game animation so it is not resampled.
			</p>
		{/if}
		{#if mode === "texture"}
			<div class="grid grid-cols-2 gap-2">
				<label class="block">
					<span class="text-xs opacity-70">Mip chain</span>
					<select class="select select-sm mt-1 w-full" bind:value={mips}>
						<option value="auto">like the original</option>
						<option value="yes">generate all mips</option>
						<option value="no">top level only</option>
					</select>
				</label>
				<label class="block">
					<span class="text-xs opacity-70">Pixel format</span>
					<select class="select select-sm mt-1 w-full font-mono" bind:value={format}>
						{#each FORMATS as f (f)}
							<option value={f}>{f || "keep the original"}</option>
						{/each}
					</select>
				</label>
			</div>
			<p class="text-xs opacity-60">
				PNG input is re-encoded (DXT via texpresso); DDS input is stored as-is when its
				format is supported. All mips are stored inline in the package and the texture is
				flagged NeverStream, so no .tfc edit is needed.
			</p>
		{:else if mode === "mesh"}
			<label class="flex items-center gap-2">
				<span class="text-xs opacity-70">LOD to replace</span>
				<input type="number" class="input input-xs w-16" min="0" bind:value={lod} />
			</label>
			<p class="text-xs opacity-60">
				glTF (.glb/.gltf), FBX (binary) or OBJ, in the layout the exporter writes: Y up,
				metres for glTF/OBJ and centimetres for FBX, materials named after the mesh's
				material slots (otherwise sections map to slots in order). Static meshes get new
				vertex/index buffers, bounds and per-triangle collision; the simple collision
				(RB_BodySetup) stays. Skeletal meshes need a skin whose joints are named like the
				existing bones; the skeleton itself is kept.
			</p>
		{:else if mode === "sound"}
			<p class="text-xs opacity-60">
				Ogg Vorbis is stored as is; WAV and Wwise .wem files are re-encoded as Ogg Vorbis
				(quality 0.7). Duration, channels and sample rate are updated from the file.
			</p>
		{:else if mode === "swf"}
			<p class="text-xs opacity-60">
				The movie's RawData is replaced by the file (a compiled .gfx, or a plain .swf).
			</p>
		{:else if mode === "raw"}
			<p class="text-xs opacity-60">
				The export's serialized payload is replaced byte for byte by the file (the inverse
				of
				<b>Extract raw</b>): use it to put back a payload you extracted and edited, or to
				copy one from another package of the same class. Nothing is validated beyond trying
				to parse it afterwards.
			</p>
		{/if}
		<div class="flex justify-end gap-2">
			<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}>
				Cancel
			</button>
			<button
				class="btn btn-sm btn-primary"
				type="submit"
				disabled={!file ||
					working ||
					(mode === "anim" &&
						(isSet || allSequences) &&
						animations.length > 0 &&
						!pairs.some((p) => p.sequence))}
			>
				Replace
			</button>
		</div>
	</form>
</Modal>
