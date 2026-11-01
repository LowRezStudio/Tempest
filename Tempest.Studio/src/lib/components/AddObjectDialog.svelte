<!--
	Add to the open package: a new object made from a file (texture, mesh,
	sound, movie) on the model of an existing object of the same kind, which
	gives it its settings (and, for meshes, skeleton and materials); a copy of
	an object from another package; or a group. Everything is an undoable,
	unsaved edit like any other.
-->
<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import ObjectPicker from "$lib/components/ObjectPicker.svelte";
	import { pickFile } from "$lib/platform/api";
	import { basename } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { SearchHit } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	type Tab = "file" | "copy" | "group";
	let tab = $state<Tab>("file");
	let file = $state("");
	let name = $state("");
	let group = $state<number | "top">("top");
	let template = $state<SearchHit | null>(null);
	let meshKind = $state<"StaticMesh" | "SkeletalMesh">("StaticMesh");
	let deep = $state(false);
	let working = $state(false);

	const groups = $derived(
		(studio.pkg?.exports ?? []).filter((e) => e.class.toLowerCase() === "package"),
	);
	const ext = $derived(file.split(".").pop()?.toLowerCase() ?? "");
	/** The kind of object the file becomes, as the class templates are searched by. */
	const isA = $derived(
		["png", "dds", "tga"].includes(ext)
			? "Texture2D"
			: ["glb", "gltf", "fbx", "obj"].includes(ext)
				? meshKind
				: ["ogg", "wav"].includes(ext)
					? "SoundNodeWave"
					: ["gfx", "swf"].includes(ext)
						? "SwfMovie"
						: "",
	);

	$effect(() => {
		if (!open) return;
		file = "";
		name = "";
		template = null;
		group = ui.addGroup ?? "top";
		tab = "file";
	});

	// A new file kind needs a template of that kind.
	$effect(() => {
		void isA;
		template = null;
	});

	async function browse() {
		const f = await pickFile("File to add", [
			{
				name: "Textures, meshes, sounds, movies",
				extensions: ["png", "dds", "glb", "gltf", "fbx", "obj", "ogg", "wav", "gfx", "swf"],
			},
		]);
		if (!f) return;
		file = f;
		if (!name.trim()) {
			name = basename(f)
				.replace(/\.[^.]+$/, "")
				.replaceAll(/[^A-Za-z0-9_]/g, "_");
		}
	}

	async function done(index: number, what: string) {
		if (!studio.pkg) return;
		studio.markDirty(studio.pkg.path);
		await studio.reloadTables();
		await studio.select(index);
		toasts.push("success", what, "Added");
		open = false;
	}

	async function submit(event: Event) {
		event.preventDefault();
		const pkg = studio.pkg;
		if (!pkg) return;
		working = true;
		try {
			if (tab === "file") {
				if (!file || !template) return;
				const r = await studio.run("Adding…", () =>
					rpc("object.add", {
						path: pkg.path,
						file,
						template: { path: template!.path, index: template!.index },
						name: name.trim() || undefined,
						outer: group,
					}),
				);
				await done(r.index, `${r.path}: ${r.message}`);
			} else if (tab === "copy") {
				if (!template) return;
				const r = await studio.run("Copying…", () =>
					rpc("object.copy", {
						src: template!.path,
						index: template!.index,
						dst: pkg.path,
						outer: group,
						deep,
					}),
				);
				for (const w of r.warnings.slice(0, 5)) toasts.push("warning", w, "Copy");
				await done(r.index, r.path);
			} else {
				const r = await rpc("object.addGroup", {
					path: pkg.path,
					name: name.trim(),
					outer: group === "top" ? undefined : group,
				});
				await done(r.index, r.path);
			}
		} catch (error) {
			toasts.error(error, "Could not add it");
		} finally {
			working = false;
		}
	}

	const ready = $derived(
		tab === "file"
			? !!file && !!isA && !!template
			: tab === "copy"
				? !!template
				: /^[A-Za-z0-9_]+$/.test(name.trim()),
	);
</script>

<Modal bind:open title="Add to {studio.pkg?.name ?? 'the package'}" width="max-w-2xl">
	<form onsubmit={submit} class="space-y-3 text-sm">
		<div role="tablist" class="tabs-box tabs tabs-xs">
			<button
				role="tab"
				type="button"
				class="tab {tab === 'file' ? 'tab-active' : ''}"
				onclick={() => (tab = "file")}>From a file</button
			>
			<button
				role="tab"
				type="button"
				class="tab {tab === 'copy' ? 'tab-active' : ''}"
				onclick={() => {
					tab = "copy";
					template = null;
				}}>Copy from another package</button
			>
			<button
				role="tab"
				type="button"
				class="tab {tab === 'group' ? 'tab-active' : ''}"
				onclick={() => (tab = "group")}>Group</button
			>
		</div>

		{#if tab === "file"}
			<div class="flex gap-1">
				<input
					class="input input-sm flex-1 font-mono text-xs"
					bind:value={file}
					placeholder="PNG/DDS, glTF/FBX/OBJ, Ogg/WAV, GFX/SWF"
				/>
				<button class="btn btn-sm" type="button" onclick={browse}>Browse…</button>
			</div>
			{#if file && !isA}
				<p class="text-error text-xs">.{ext} files cannot be added.</p>
			{/if}
			{#if ["glb", "gltf", "fbx", "obj"].includes(ext)}
				<div class="flex gap-4 text-xs">
					<label class="flex items-center gap-2"
						><input
							type="radio"
							class="radio radio-xs"
							value="StaticMesh"
							bind:group={meshKind}
						/> Static mesh (props, effects)</label
					>
					<label class="flex items-center gap-2"
						><input
							type="radio"
							class="radio radio-xs"
							value="SkeletalMesh"
							bind:group={meshKind}
						/> Skeletal mesh (characters, weapons)</label
					>
				</div>
			{/if}
			{#if isA}
				<div class="flex flex-col gap-1">
					<span class="text-xs opacity-70">
						Made like: a {isA} it copies its settings from{isA === "SkeletalMesh"
							? " (and its skeleton: pick one with the bones your file uses)"
							: isA === "StaticMesh"
								? " (and its materials and collision)"
								: ""}. Its materials, textures and skeleton stay where they are and
						are referenced from there.
					</span>
					<ObjectPicker
						{isA}
						local={[isA.toLowerCase()]}
						value={template}
						onpick={(h) => (template = h)}
					/>
				</div>
			{/if}
		{:else if tab === "copy"}
			<ObjectPicker
				value={template}
				onpick={(h) => (template = h)}
				placeholder="the object to copy"
			/>
			<label class="flex items-start gap-2 text-xs">
				<input type="checkbox" class="checkbox checkbox-xs mt-0.5" bind:checked={deep} />
				<span
					>Also copy everything it uses from its package (materials, textures…). Otherwise
					those references are cleared; its own sub-objects come along either way.</span
				>
			</label>
		{/if}

		<div class="grid gap-3 sm:grid-cols-2">
			{#if tab !== "copy"}
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">Name</span>
					<input
						class="input input-sm w-full font-mono"
						bind:value={name}
						placeholder={tab === "group" ? "textures" : "T_MyTexture"}
						spellcheck="false"
					/>
				</label>
			{/if}
			<label class="flex flex-col gap-1">
				<span class="text-xs opacity-70">In group</span>
				<select class="select select-sm w-full font-mono" bind:value={group}>
					<option value="top">(top level)</option>
					{#each groups as g (g.i)}<option value={g.i}>{g.path}</option>{/each}
				</select>
			</label>
		</div>
		{#if template && tab !== "group"}
			<p class="text-xs opacity-60">
				From <span class="font-mono">{template.package} · {template.object}</span>
			</p>
		{/if}
		<div class="flex justify-end gap-2">
			<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}
				>Cancel</button
			>
			<button class="btn btn-primary btn-sm" disabled={!ready || working}>Add</button>
		</div>
	</form>
</Modal>
