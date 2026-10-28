<!--
	Add an external image to a movie (`SwfDoc.addImage`): a PNG or DDS becomes
	a Texture2D of the package next to the movie's other images, the movie
	gets a `DefineExternalImage2` naming it and, by default, a shape drawing
	it, placed on the open timeline frame. Pending edits are written with it
	(one undo step).
-->
<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { pickFile } from "$lib/platform/api";
	import { studio } from "$lib/studio/state.svelte";
	import type { SwfDoc } from "$lib/studio/swf.svelte";

	let { open = $bindable(false), doc }: { open?: boolean; doc: SwfDoc } = $props();

	let file = $state("");
	let name = $state("");
	let shape = $state(true);
	let place = $state(true);
	let x = $state(0);
	let y = $state(0);
	let instance = $state("");
	let working = $state(false);

	const movie = $derived(studio.pkg?.exports[doc.index]?.name ?? "Movie");
	const stem = $derived(
		file
			.replace(/^.*[\\/]/, "")
			.replace(/\.[a-z0-9]+$/i, "")
			.replaceAll(/[^A-Za-z0-9_]/g, "_"),
	);
	const where = $derived(
		doc.sprite === null
			? null
			: `${doc.sprite === 0 ? "the root timeline" : doc.label(doc.sprite)}, frame ${doc.frame + 1}`,
	);

	$effect(() => {
		if (open) {
			file = "";
			name = "";
			instance = "";
			working = false;
		}
	});

	async function browse() {
		const f = await pickFile("Image to add", [{ name: "Images", extensions: ["png", "dds"] }]);
		if (f) file = f;
	}

	async function submit(event: Event) {
		event.preventDefault();
		if (!file) return;
		working = true;
		const r = await doc.addImage({
			file,
			name: name.trim() || undefined,
			shape,
			place:
				shape && place && where ? { x, y, name: instance.trim() || undefined } : undefined,
		});
		working = false;
		if (r) open = false;
	}
</script>

<Modal bind:open title="Add an image" width="max-w-lg">
	<form onsubmit={submit} class="space-y-3 text-sm">
		<div class="flex gap-1">
			<input
				class="input input-sm flex-1 font-mono text-xs"
				bind:value={file}
				placeholder="PNG or DDS file"
			/>
			<button class="btn btn-sm" type="button" onclick={browse}>Browse…</button>
		</div>
		<label class="block">
			<span class="text-xs opacity-70">Texture name</span>
			<input
				class="input input-sm mt-1 w-full font-mono text-xs"
				bind:value={name}
				placeholder={stem ? `${movie}_${stem}` : `${movie}_<file name>`}
			/>
		</label>
		<label class="flex items-center gap-2">
			<input type="checkbox" class="checkbox checkbox-sm" bind:checked={shape} />
			<span>Make a shape that draws it</span>
		</label>
		{#if shape && where}
			<label class="flex items-center gap-2">
				<input type="checkbox" class="checkbox checkbox-sm" bind:checked={place} />
				<span>Place it on {where}, above everything</span>
			</label>
			{#if place}
				<div class="grid grid-cols-[auto_1fr_auto_1fr] items-center gap-2 pl-6 text-xs">
					<span class="opacity-70">x</span>
					<input type="number" step="0.05" class="input input-xs" bind:value={x} />
					<span class="opacity-70">y</span>
					<input type="number" step="0.05" class="input input-xs" bind:value={y} />
					<span class="opacity-70">name</span>
					<input
						class="input input-xs col-span-3 font-mono"
						bind:value={instance}
						placeholder="instance name (optional, for scripts)"
					/>
				</div>
			{/if}
		{/if}
		<p class="text-xs opacity-60">
			The picture becomes a Texture2D next to the movie's other images (same settings; PNG is
			encoded like them, DXT5 when it has transparency) and is listed in the movie's
			references; the movie gets a <code>DefineExternalImage2</code> naming it, drawn at the
			picture's size.
			{#if doc.pending.length > 0}
				The {doc.pending.length} pending edit{doc.pending.length > 1 ? "s are" : " is"} written
				with it.
			{/if}
			Undo removes all of it.
		</p>
		<div class="flex justify-end gap-2">
			<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}>
				Cancel
			</button>
			<button class="btn btn-sm btn-primary" type="submit" disabled={!file || working}>
				{#if working}<span class="loading loading-xs loading-spinner"></span>{/if}
				Add
			</button>
		</div>
	</form>
</Modal>
