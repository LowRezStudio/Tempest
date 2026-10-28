<!--
	An external image of a movie: a thumbnail of the Texture2D it names and
	what can be done with the picture (replace it with a PNG or DDS, export
	it, go to the texture). The movie keeps drawing the image at the size its
	tag states, so a sharper picture of the same aspect ratio stays in place.
-->
<script lang="ts">
	import { Download, Link2, Upload } from "@lucide/svelte";
	import { blobUrl, rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { textureFor, type SwfDoc } from "$lib/studio/swf.svelte";
	import type { SwfImage } from "../../../../electron/src/ipc/contract";

	interface Props {
		doc: SwfDoc;
		image: SwfImage;
		/** One table row's worth: small thumbnail, icon buttons. */
		compact?: boolean;
	}
	const { doc, image, compact = false }: Props = $props();

	const texture = $derived(textureFor(doc.value("image", image.tag, image.fileName)));
	let thumb = $state<string | null>(null);
	let failed = $state(false);

	$effect(() => {
		const index = texture;
		void doc.stamp;
		failed = false;
		if (index === null) {
			thumb = null;
			return;
		}
		let url: string | null = null;
		let live = true;
		rpc("texture.preview", { path: doc.path, index, maxSize: compact ? 64 : 256 })
			.then((r) => {
				if (!live) return;
				url = blobUrl(r.png, "image/png");
				thumb = url;
			})
			.catch(() => {
				if (live) failed = true;
			});
		return () => {
			live = false;
			if (url) URL.revokeObjectURL(url);
		};
	});
</script>

{#snippet picture(size: string)}
	<div
		class="checker flex shrink-0 items-center justify-center overflow-hidden rounded {size}"
		title={texture === null
			? "no Texture2D of that name in this package"
			: `${image.width}×${image.height} in the movie`}
	>
		{#if thumb}
			<img src={thumb} alt={image.fileName} class="max-h-full max-w-full object-contain" />
		{:else if texture === null || failed}
			<span class="text-[10px] opacity-40">?</span>
		{:else}
			<span class="loading loading-xs loading-spinner opacity-40"></span>
		{/if}
	</div>
{/snippet}

{#if compact}
	<div class="flex items-center gap-1">
		{@render picture("h-8 w-8")}
		<button
			class="btn btn-ghost btn-xs"
			disabled={texture === null}
			title="Replace the picture (PNG or DDS)"
			onclick={() => texture !== null && doc.replaceImage(texture, image)}
			><Upload size={11} /></button
		>
		<button
			class="btn btn-ghost btn-xs"
			disabled={texture === null}
			title="Export as PNG"
			onclick={() => texture !== null && doc.exportImage(texture, image, "png")}
			><Download size={11} /></button
		>
		<button
			class="btn btn-ghost btn-xs"
			disabled={texture === null}
			title="Go to the texture"
			onclick={() => texture !== null && studio.select(texture)}><Link2 size={11} /></button
		>
	</div>
{:else}
	<div class="flex flex-col gap-1">
		{@render picture("h-32 w-full")}
		<div class="flex flex-wrap gap-1">
			<button
				class="btn btn-xs"
				disabled={texture === null}
				title="Store a PNG or DDS in the texture. PNG is encoded in the texture's format (DXT1 becomes DXT5 when the picture has transparency); the movie keeps drawing it at {image.width}×{image.height}."
				onclick={() => texture !== null && doc.replaceImage(texture, image)}
				><Upload size={11} /> Replace…</button
			>
			<button
				class="btn btn-ghost btn-xs"
				disabled={texture === null}
				onclick={() => texture !== null && doc.exportImage(texture, image, "png")}
				><Download size={11} /> PNG</button
			>
			<button
				class="btn btn-ghost btn-xs"
				disabled={texture === null}
				onclick={() => texture !== null && doc.exportImage(texture, image, "dds")}
				><Download size={11} /> DDS</button
			>
			<button
				class="btn btn-ghost btn-xs"
				disabled={texture === null}
				title="Go to the texture"
				onclick={() => texture !== null && studio.select(texture)}
				><Link2 size={11} /> Texture</button
			>
		</div>
	</div>
{/if}

<style>
	.checker {
		background-color: var(--color-base-300);
		background-image:
			linear-gradient(45deg, rgb(127 127 127 / 0.12) 25%, transparent 25%),
			linear-gradient(-45deg, rgb(127 127 127 / 0.12) 25%, transparent 25%),
			linear-gradient(45deg, transparent 75%, rgb(127 127 127 / 0.12) 75%),
			linear-gradient(-45deg, transparent 75%, rgb(127 127 127 / 0.12) 75%);
		background-size: 12px 12px;
		background-position:
			0 0,
			0 6px,
			6px -6px,
			-6px 0;
	}
</style>
