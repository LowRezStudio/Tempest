<script lang="ts">
	import { onDestroy } from "svelte";
	import { bytes } from "$lib/studio/format";
	import { blobUrl, rpc } from "$lib/studio/rpc";
	import type { MediaPreview } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let preview = $state<MediaPreview | null>(null);
	let url = $state<string | null>(null);
	let error = $state<string | null>(null);

	$effect(() => {
		void (async () => {
			try {
				const r = await rpc("media.preview", { path, index });
				preview = r;
				if (r.kind === "sound") url = blobUrl(r.ogg, "audio/ogg");
			} catch (error_) {
				error = error_ instanceof Error ? error_.message : String(error_);
			}
		})();
	});

	onDestroy(() => {
		if (url) URL.revokeObjectURL(url);
	});
</script>

<div class="p-4 text-sm">
	{#if error}
		<p class="text-error">{error}</p>
	{:else if !preview}
		<span class="loading loading-sm loading-spinner"></span>
	{:else if preview.kind === "sound"}
		<audio controls src={url ?? undefined} class="w-full"></audio>
		<p class="mt-2 font-mono text-xs opacity-70">
			Ogg Vorbis · {bytes(preview.bytes)}
			{#if preview.channels}· {preview.channels} ch · {preview.sampleRate} Hz · {preview.duration?.toFixed(
					2,
				)} s{/if}
		</p>
	{:else}
		<p class="font-mono text-xs">
			Scaleform movie · {bytes(preview.bytes)} · signature {preview.signature}
		</p>
		<p class="mt-2 opacity-70">
			Export the .gfx to inspect it; replace it with a compiled .gfx or .swf.
		</p>
	{/if}
</div>
