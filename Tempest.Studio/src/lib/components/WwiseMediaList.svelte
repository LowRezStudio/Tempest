<script lang="ts">
	import { Download, Pause, Pencil, Play, Upload } from "@lucide/svelte";
	import { onDestroy } from "svelte";
	import SoundEditor from "$lib/components/SoundEditor.svelte";
	import { pickFile, pickSave } from "$lib/platform/api";
	import { formatTime } from "$lib/studio/audio";
	import { basename, bytes } from "$lib/studio/format";
	import { blobUrl, rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { WwiseMedia } from "../../../electron/src/ipc/contract";

	interface Props {
		/** The .pck file. */
		pck: string;
		media: WwiseMedia[];
		/** Called after a media file was replaced. */
		onchanged?: () => void;
		/** Highlighted ids (what an event plays). */
		highlight?: Set<number>;
	}
	const { pck, media, onchanged, highlight }: Props = $props();

	let playingId = $state<number | null>(null);
	let audio: HTMLAudioElement | null = null;
	let url: string | null = null;
	let loading = $state<number | null>(null);
	let editing = $state<{ id: number; ogg: Uint8Array } | null>(null);
	let busy = $state(false);

	function release() {
		if (audio) {
			audio.pause();
			audio.src = "";
			audio = null;
		}
		if (url) URL.revokeObjectURL(url);
		url = null;
		playingId = null;
	}

	async function fetchOgg(id: number): Promise<Uint8Array> {
		const r = await rpc("wwise.preview", { path: pck, id });
		return r.ogg;
	}

	async function play(m: WwiseMedia) {
		if (playingId === m.id) {
			release();
			return;
		}
		release();
		loading = m.id;
		try {
			const ogg = await fetchOgg(m.id);
			url = blobUrl(ogg, "audio/ogg");
			audio = new Audio(url);
			audio.onended = () => release();
			playingId = m.id;
			await audio.play();
		} catch (error) {
			toasts.error(error, "Cannot play");
			release();
		} finally {
			loading = null;
		}
	}

	async function exportMedia(m: WwiseMedia) {
		const out = await pickSave(`Export media ${m.id}`, `${m.id}.ogg`, [
			{ name: "Ogg Vorbis", extensions: ["ogg"] },
			{ name: "WAV (16-bit PCM)", extensions: ["wav"] },
			{ name: "Wwise media (as stored)", extensions: ["wem"] },
		]);
		if (!out) return;
		try {
			const r = await rpc("wwise.export", { path: pck, id: m.id, out });
			toasts.push("success", `${bytes(r.bytes)} → ${out}`, "Exported");
		} catch (error) {
			toasts.error(error, "Export failed");
		}
	}

	/** A replaced media file is an unsaved edit of the .pck, like a package's. */
	function replaced(n: number) {
		studio.markDirty(pck);
		toasts.push(
			"success",
			`${bytes(n)}; ${basename(pck)} is unsaved until you save (or build the mod)`,
			"Media replaced",
		);
		onchanged?.();
	}

	async function replaceMedia(m: WwiseMedia) {
		const file = await pickFile(`Replace media ${m.id}`, [
			{ name: "Audio (Ogg Vorbis, WAV, Wwise .wem)", extensions: ["ogg", "wav", "wem"] },
		]);
		if (!file) return;
		busy = true;
		release();
		try {
			const r = await studio.run("Encoding…", () =>
				rpc("wwise.import", { path: pck, id: m.id, file, quality: 0.7 }),
			);
			replaced(r.bytes);
		} catch (error) {
			toasts.error(error, "Replace failed");
		} finally {
			busy = false;
		}
	}

	async function edit(m: WwiseMedia) {
		release();
		loading = m.id;
		try {
			editing = { id: m.id, ogg: await fetchOgg(m.id) };
		} catch (error) {
			toasts.error(error, "Cannot decode");
		} finally {
			loading = null;
		}
	}

	async function applyEdit(payload: {
		pcm: string;
		channels: number;
		sampleRate: number;
		quality: number;
	}) {
		if (!editing) return;
		const id = editing.id;
		const r = await studio.run("Encoding…", () =>
			rpc("wwise.replacePcm", { path: pck, id, ...payload }),
		);
		editing = null;
		replaced(r.bytes);
	}

	onDestroy(release);
</script>

<div class="overflow-auto">
	<table class="table-xs table font-mono">
		<thead>
			<tr><th></th><th>id</th><th>format</th><th>length</th><th>size</th><th></th></tr>
		</thead>
		<tbody>
			{#each media as m (m.id)}
				<tr class={highlight?.has(m.id) ? "bg-primary/10" : ""}>
					<td class="w-8">
						<button
							class="btn btn-ghost btn-xs"
							onclick={() => play(m)}
							disabled={busy || !!m.error}
							title={playingId === m.id ? "Stop" : "Play"}
						>
							{#if loading === m.id}<span class="loading loading-xs loading-spinner"
								></span>{:else if playingId === m.id}<Pause size={12} />{:else}<Play
									size={12}
								/>{/if}
						</button>
					</td>
					<td class="selectable"
						>{m.id}{#if m.streamed}<span class="ml-1 opacity-50">stream</span>{/if}</td
					>
					<td class="opacity-80"
						>{m.error ?? `${m.codec} ${m.sampleRate} Hz ${m.channels} ch`}</td
					>
					<td>{formatTime(m.duration)}</td>
					<td class="opacity-70">{bytes(m.bytes)}</td>
					<td class="whitespace-nowrap">
						<button
							class="btn btn-ghost btn-xs"
							onclick={() => exportMedia(m)}
							title="Export as Ogg, WAV or .wem"><Download size={12} /></button
						>
						<button
							class="btn btn-ghost btn-xs"
							onclick={() => replaceMedia(m)}
							disabled={busy}
							title="Replace with an Ogg, WAV or .wem file"
							><Upload size={12} /></button
						>
						<button
							class="btn btn-ghost btn-xs"
							onclick={() => edit(m)}
							disabled={busy || !!m.error}
							title="Trim, fade, gain, normalise"><Pencil size={12} /></button
						>
					</td>
				</tr>
				{#if editing?.id === m.id}
					<tr>
						<td colspan="6" class="p-1">
							<SoundEditor
								ogg={editing.ogg}
								name={`media ${m.id}`}
								sampleRate={m.sampleRate}
								onapply={applyEdit}
								onclose={() => (editing = null)}
							/>
						</td>
					</tr>
				{/if}
			{/each}
		</tbody>
	</table>
	{#if media.length === 0}
		<p class="p-2 text-xs opacity-60">No media.</p>
	{/if}
</div>
