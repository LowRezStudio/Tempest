<script lang="ts">
	import { Pencil } from "@lucide/svelte";
	import { onDestroy } from "svelte";
	import SoundEditor from "$lib/components/SoundEditor.svelte";
	import { bytes } from "$lib/studio/format";
	import { blobUrl, rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { MediaPreview } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let preview = $state<MediaPreview | null>(null);
	let url = $state<string | null>(null);
	let error = $state<string | null>(null);
	let editing = $state(false);
	let version = $state(0);

	$effect(() => {
		void version;
		void (async () => {
			try {
				const r = await rpc("media.preview", { path, index });
				preview = r;
				if (url) URL.revokeObjectURL(url);
				url = r.kind === "sound" ? blobUrl(r.ogg, "audio/ogg") : null;
			} catch (error_) {
				error = error_ instanceof Error ? error_.message : String(error_);
			}
		})();
	});

	async function applyEdit(payload: {
		pcm: string;
		channels: number;
		sampleRate: number;
		quality: number;
	}) {
		const r = await studio.run("Encoding…", () =>
			rpc("sound.replacePcm", { path, index, ...payload }),
		);
		toasts.push(
			"success",
			`${bytes(r.bytes)} of Ogg Vorbis, ${r.duration.toFixed(2)} s. Save the package to write it.`,
			"Sound replaced",
		);
		studio.markDirty(path);
		editing = false;
		version++;
		await studio.select(index);
	}

	onDestroy(() => {
		if (url) URL.revokeObjectURL(url);
	});
</script>

<div class="space-y-2 p-4 text-sm">
	{#if error}
		<p class="text-error">{error}</p>
	{:else if !preview}
		<span class="loading loading-sm loading-spinner"></span>
	{:else if preview.kind === "sound"}
		{#key version}
			<audio controls src={url ?? undefined} class="w-full"></audio>
		{/key}
		<div class="flex items-center gap-2">
			<p class="font-mono text-xs opacity-70">
				Ogg Vorbis · {bytes(preview.bytes)}
				{#if preview.channels}· {preview.channels} ch · {preview.sampleRate} Hz · {preview.duration?.toFixed(
						2,
					)} s{/if}
			</p>
			<div class="flex-1"></div>
			{#if !editing}
				<button
					class="btn btn-xs"
					onclick={() => (editing = true)}
					title="Trim, fade, gain, normalise, then re-encode"
				>
					<Pencil size={12} /> Edit
				</button>
			{/if}
		</div>
		{#if editing}
			<SoundEditor
				ogg={preview.ogg}
				name="sound"
				sampleRate={preview.sampleRate ?? undefined}
				onapply={applyEdit}
				onclose={() => (editing = false)}
			/>
		{/if}
	{:else}
		<p class="font-mono text-xs">
			Scaleform movie · {bytes(preview.bytes)} · signature {preview.signature}
		</p>
		<p class="mt-2 opacity-70">
			Export the .gfx to inspect it; replace it with a compiled .gfx or .swf.
		</p>
	{/if}
</div>
