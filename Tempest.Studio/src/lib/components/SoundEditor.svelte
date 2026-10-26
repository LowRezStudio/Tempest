<script lang="ts">
	import { Pause, Play, Scissors, Square, X } from "@lucide/svelte";
	import { onDestroy } from "svelte";
	import {
		applyEdits,
		audioContext,
		decodeClip,
		duration,
		formatTime,
		NO_EDITS,
		peaks,
		pcmPayload,
		toBuffer,
		type Clip,
		type Edits,
	} from "$lib/studio/audio";
	import { toasts } from "$lib/studio/toast.svelte";

	interface Props {
		/** The sound as Ogg Vorbis. */
		ogg: Uint8Array;
		name: string;
		/** The stored sample rate (decoding keeps it). */
		sampleRate?: number;
		/** Encode and store the edited audio; resolves when done. */
		onapply: (payload: {
			pcm: string;
			channels: number;
			sampleRate: number;
			quality: number;
		}) => Promise<void>;
		onclose: () => void;
	}
	const { ogg, name, sampleRate, onapply, onclose }: Props = $props();

	let source = $state<Clip | null>(null);
	let error = $state<string | null>(null);
	let edits = $state<Edits>({ ...NO_EDITS });
	let quality = $state(0.7);
	let working = $state(false);
	let canvas = $state<HTMLCanvasElement | null>(null);
	let playing = $state(false);
	let playhead = $state(0);
	let node: AudioBufferSourceNode | null = null;
	let playStart = 0;
	let playOffset = 0;
	let raf = 0;
	/** Selection in seconds (on the source timeline). */
	let selection = $state<[number, number] | null>(null);
	let dragging: number | null = null;

	// The result of the edits, recomputed when they change.
	const edited = $derived.by(() => (source ? applyEdits(source, edits) : null));
	const total = $derived(source ? duration(source) : 0);
	const outDuration = $derived(edited ? duration(edited) : 0);

	$effect(() => {
		void (async () => {
			try {
				source = await decodeClip(ogg, sampleRate);
			} catch (error_) {
				error = error_ instanceof Error ? error_.message : String(error_);
			}
		})();
	});

	function draw() {
		const c = canvas;
		const clip = source;
		if (!c || !clip) return;
		const w = c.clientWidth;
		const h = c.clientHeight;
		if (c.width !== w * devicePixelRatio || c.height !== h * devicePixelRatio) {
			c.width = w * devicePixelRatio;
			c.height = h * devicePixelRatio;
		}
		const ctx = c.getContext("2d");
		if (!ctx) return;
		ctx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
		const style = getComputedStyle(c);
		ctx.fillStyle = style.getPropertyValue("--color-base-300") || "#222";
		ctx.fillRect(0, 0, w, h);
		const gain = 10 ** (edits.gainDb / 20);
		const lanes = clip.channels.length;
		const laneH = h / lanes;
		for (const [i, samples] of clip.channels.entries()) {
			const p = peaks(samples, w);
			const mid = laneH * i + laneH / 2;
			ctx.fillStyle = style.getPropertyValue("--color-primary") || "#58f";
			for (let x = 0; x < w; x++) {
				const lo = Math.max(-1, Math.min(1, p[x * 2] * gain));
				const hi = Math.max(-1, Math.min(1, p[x * 2 + 1] * gain));
				const y1 = mid - (hi * laneH) / 2;
				const y2 = mid - (lo * laneH) / 2;
				ctx.fillRect(x, y1, 1, Math.max(1, y2 - y1));
			}
			ctx.fillStyle = "rgba(128,128,128,0.4)";
			ctx.fillRect(0, mid, w, 1);
		}
		// Trimmed-away parts.
		if (edits.trim && total > 0) {
			ctx.fillStyle = "rgba(0,0,0,0.5)";
			const a = (edits.trim[0] / total) * w;
			const b = (edits.trim[1] / total) * w;
			ctx.fillRect(0, 0, a, h);
			ctx.fillRect(b, 0, w - b, h);
		}
		if (selection && total > 0) {
			ctx.fillStyle = "rgba(255,255,255,0.18)";
			const a = (selection[0] / total) * w;
			const b = (selection[1] / total) * w;
			ctx.fillRect(a, 0, Math.max(1, b - a), h);
		}
		if (playing && total > 0) {
			const start = edits.trim ? edits.trim[0] : 0;
			const x = ((start + playhead) / total) * w;
			ctx.fillStyle = style.getPropertyValue("--color-error") || "#f55";
			ctx.fillRect(x, 0, 1.5, h);
		}
	}

	$effect(() => {
		// Track everything the drawing depends on.
		void source;
		void edits.trim;
		void edits.gainDb;
		void selection;
		void playhead;
		void playing;
		void canvas;
		draw();
	});

	function timeAt(event: MouseEvent): number {
		const c = canvas;
		if (!c || total <= 0) return 0;
		const r = c.getBoundingClientRect();
		return Math.max(0, Math.min(total, ((event.clientX - r.left) / r.width) * total));
	}

	function down(event: MouseEvent) {
		dragging = timeAt(event);
		selection = null;
	}

	function move(event: MouseEvent) {
		if (dragging === null) return;
		const t = timeAt(event);
		selection = [Math.min(dragging, t), Math.max(dragging, t)];
	}

	function up(event: MouseEvent) {
		if (dragging === null) return;
		const t = timeAt(event);
		selection =
			Math.abs(t - dragging) < 0.01 ? null : [Math.min(dragging, t), Math.max(dragging, t)];
		dragging = null;
	}

	function trimToSelection() {
		if (!selection) return;
		edits = { ...edits, trim: [selection[0], selection[1]] };
		selection = null;
	}

	function clearTrim() {
		edits = { ...edits, trim: null };
	}

	function stop() {
		if (node) {
			node.onended = null;
			try {
				node.stop();
			} catch {
				// Already stopped.
			}
			node.disconnect();
			node = null;
		}
		cancelAnimationFrame(raf);
		playing = false;
		playhead = 0;
	}

	function play() {
		const clip = edited;
		if (!clip) return;
		stop();
		const ctx = audioContext();
		const buffer = toBuffer(clip);
		node = ctx.createBufferSource();
		node.buffer = buffer;
		node.connect(ctx.destination);
		playOffset = 0;
		playStart = ctx.currentTime;
		node.onended = () => stop();
		node.start();
		playing = true;
		const tick = () => {
			if (!playing) return;
			playhead = playOffset + (ctx.currentTime - playStart);
			raf = requestAnimationFrame(tick);
		};
		raf = requestAnimationFrame(tick);
	}

	async function apply() {
		const clip = edited;
		if (!clip) return;
		working = true;
		stop();
		try {
			await onapply({ ...pcmPayload(clip), quality });
		} catch (error_) {
			toasts.error(error_, "Sound not replaced");
		} finally {
			working = false;
		}
	}

	onDestroy(stop);
</script>

<div class="border-base-300 bg-base-200 rounded border p-2 text-xs">
	<div class="mb-1 flex items-center gap-2">
		<span class="font-semibold">Edit {name}</span>
		{#if source}
			<span class="opacity-60"
				>{source.channels.length} ch · {source.sampleRate} Hz · {formatTime(total)}</span
			>
		{/if}
		<div class="flex-1"></div>
		<button class="btn btn-ghost btn-xs" onclick={onclose} title="Close the editor"
			><X size={12} /></button
		>
	</div>
	{#if error}
		<p class="text-error">{error}</p>
	{:else if !source}
		<span class="loading loading-xs loading-spinner"></span> decoding…
	{:else}
		<canvas
			bind:this={canvas}
			class="bg-base-300 h-32 w-full cursor-crosshair rounded"
			onmousedown={down}
			onmousemove={move}
			onmouseup={up}
			onmouseleave={up}
		></canvas>
		<div class="mt-1 flex flex-wrap items-center gap-2">
			{#if playing}
				<button class="btn btn-xs" onclick={stop}><Square size={11} /> Stop</button>
			{:else}
				<button class="btn btn-xs" onclick={play}><Play size={11} /> Play result</button>
			{/if}
			<span class="opacity-60"
				>{selection
					? `selection ${formatTime(selection[0])} – ${formatTime(selection[1])}`
					: "drag on the waveform to select"}</span
			>
			<button class="btn btn-xs" onclick={trimToSelection} disabled={!selection}
				><Scissors size={11} /> Trim to selection</button
			>
			{#if edits.trim}
				<button class="btn btn-xs btn-ghost" onclick={clearTrim}
					>Undo trim ({formatTime(edits.trim[0])} – {formatTime(edits.trim[1])})</button
				>
			{/if}
		</div>
		<div class="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 md:grid-cols-3">
			<label class="flex items-center gap-2">
				<span class="w-16 opacity-70">Gain</span>
				<input
					type="range"
					class="range range-xs flex-1"
					min="-24"
					max="24"
					step="0.5"
					bind:value={edits.gainDb}
				/>
				<span class="w-12 text-right font-mono"
					>{edits.gainDb > 0 ? "+" : ""}{edits.gainDb} dB</span
				>
			</label>
			<label class="flex items-center gap-2">
				<span class="w-16 opacity-70">Fade in</span>
				<input
					type="number"
					class="input input-xs w-20"
					min="0"
					step="0.05"
					bind:value={edits.fadeIn}
				/>
				<span class="opacity-60">s</span>
			</label>
			<label class="flex items-center gap-2">
				<span class="w-16 opacity-70">Fade out</span>
				<input
					type="number"
					class="input input-xs w-20"
					min="0"
					step="0.05"
					bind:value={edits.fadeOut}
				/>
				<span class="opacity-60">s</span>
			</label>
			<label class="flex items-center gap-2">
				<input
					type="checkbox"
					class="checkbox checkbox-xs"
					bind:checked={edits.normalize}
				/>
				Normalise to 0 dB
			</label>
			<label class="flex items-center gap-2">
				<span class="w-16 opacity-70">Channels</span>
				<select class="select select-xs" bind:value={edits.channels}>
					<option value={0}>keep ({source.channels.length})</option>
					<option value={1}>mono</option>
					<option value={2}>stereo</option>
				</select>
			</label>
			<label class="flex items-center gap-2">
				<span class="w-16 opacity-70">Quality</span>
				<input
					type="range"
					class="range range-xs flex-1"
					min="0"
					max="1"
					step="0.1"
					bind:value={quality}
				/>
				<span class="w-8 text-right font-mono">{quality.toFixed(1)}</span>
			</label>
		</div>
		<div class="mt-2 flex items-center gap-2">
			<span class="opacity-60"
				>result: {formatTime(outDuration)}, {edited?.channels.length} ch</span
			>
			<div class="flex-1"></div>
			<button class="btn btn-xs btn-primary" onclick={apply} disabled={working || !edited}>
				{#if working}<span class="loading loading-xs loading-spinner"></span>{/if}
				Apply and replace
			</button>
		</div>
	{/if}
</div>
