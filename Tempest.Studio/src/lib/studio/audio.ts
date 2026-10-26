/** Audio helpers for the sound editor: decoding through Web Audio, edits on float samples, and the PCM payload the backend encodes. */

export interface Clip {
	sampleRate: number;
	/** One Float32Array per channel. */
	channels: Float32Array[];
}

let context: AudioContext | null = null;

export function audioContext(): AudioContext {
	context ??= new AudioContext();
	return context;
}

/**
 * Decode an Ogg Vorbis (or WAV) file with the browser's decoder, at `sampleRate` when known
 * (the shared context would resample everything to its own rate).
 */
export async function decodeClip(bytes: Uint8Array, sampleRate?: number): Promise<Clip> {
	const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
	const decoder =
		sampleRate && sampleRate >= 3000 && sampleRate <= 384_000
			? new OfflineAudioContext({ numberOfChannels: 2, length: 1, sampleRate })
			: audioContext();
	const decoded = await decoder.decodeAudioData(buffer as ArrayBuffer);
	const channels: Float32Array[] = [];
	for (let c = 0; c < decoded.numberOfChannels; c++) {
		channels.push(new Float32Array(decoded.getChannelData(c)));
	}
	return { sampleRate: decoded.sampleRate, channels };
}

export function frames(clip: Clip): number {
	return clip.channels[0]?.length ?? 0;
}

export function duration(clip: Clip): number {
	return clip.sampleRate ? frames(clip) / clip.sampleRate : 0;
}

export interface Edits {
	/** Keep only this range, in seconds. */
	trim: [number, number] | null;
	fadeIn: number;
	fadeOut: number;
	/** Decibels. */
	gainDb: number;
	normalize: boolean;
	/** 0 keeps the channel count. */
	channels: 0 | 1 | 2;
}

export const NO_EDITS: Edits = {
	trim: null,
	fadeIn: 0,
	fadeOut: 0,
	gainDb: 0,
	normalize: false,
	channels: 0,
};

/** Apply the edits to a copy of the clip (what the backend will encode). */
export function applyEdits(clip: Clip, e: Edits): Clip {
	const rate = clip.sampleRate;
	let channels = clip.channels.map((c) => new Float32Array(c));
	if (e.trim) {
		const n = channels[0].length;
		const a = Math.min(n, Math.max(0, Math.round(e.trim[0] * rate)));
		const b = Math.min(n, Math.max(a, Math.round(e.trim[1] * rate)));
		channels = channels.map((c) => c.slice(a, b));
	}
	const n = channels[0]?.length ?? 0;
	if (e.fadeIn > 0 && n > 0) {
		const len = Math.max(1, Math.min(n, Math.floor(e.fadeIn * rate)));
		for (const c of channels) for (let i = 0; i < len; i++) c[i] *= i / len;
	}
	if (e.fadeOut > 0 && n > 0) {
		const len = Math.max(1, Math.min(n, Math.floor(e.fadeOut * rate)));
		for (const c of channels) for (let i = 0; i < len; i++) c[n - len + i] *= 1 - (i + 1) / len;
	}
	let gain = 10 ** (e.gainDb / 20);
	if (e.normalize) {
		let peak = 0;
		for (const c of channels) for (const s of c) peak = Math.max(peak, Math.abs(s));
		if (peak > 1e-6) gain *= 1 / peak;
	}
	if (Math.abs(gain - 1) > 1e-6) {
		for (const c of channels) {
			for (let i = 0; i < c.length; i++) c[i] = Math.max(-1, Math.min(1, c[i] * gain));
		}
	}
	if (e.channels === 1 && channels.length > 1) {
		const mono = new Float32Array(n);
		for (const c of channels) for (let i = 0; i < n; i++) mono[i] += c[i] / channels.length;
		channels = [mono];
	} else if (e.channels === 2 && channels.length === 1) {
		channels = [channels[0], new Float32Array(channels[0])];
	}
	return { sampleRate: rate, channels };
}

/** Interleaved 16-bit PCM, base64, for `sound.replacePcm` / `wwise.replacePcm`. */
export function pcmPayload(clip: Clip): { pcm: string; channels: number; sampleRate: number } {
	const n = frames(clip);
	const ch = clip.channels.length;
	const out = new Uint8Array(n * ch * 2);
	const view = new DataView(out.buffer);
	let o = 0;
	for (let i = 0; i < n; i++) {
		for (let c = 0; c < ch; c++) {
			const v = Math.max(-1, Math.min(1, clip.channels[c][i]));
			view.setInt16(o, Math.round(v * 32767), true);
			o += 2;
		}
	}
	let s = "";
	const chunk = 0x8000;
	for (let i = 0; i < out.length; i += chunk) {
		s += String.fromCharCode.apply(null, out.subarray(i, i + chunk) as unknown as number[]);
	}
	return { pcm: btoa(s), channels: ch, sampleRate: clip.sampleRate };
}

/** Min/max per pixel column of one channel. */
export function peaks(samples: Float32Array, columns: number): Float32Array {
	const out = new Float32Array(columns * 2);
	const per = samples.length / columns;
	for (let x = 0; x < columns; x++) {
		const a = Math.floor(x * per);
		const b = Math.min(samples.length, Math.max(a + 1, Math.floor((x + 1) * per)));
		let lo = 1;
		let hi = -1;
		for (let i = a; i < b; i++) {
			const s = samples[i];
			if (s < lo) lo = s;
			if (s > hi) hi = s;
		}
		out[x * 2] = lo;
		out[x * 2 + 1] = hi;
	}
	return out;
}

/** An AudioBuffer of a clip, for playback. */
export function toBuffer(clip: Clip): AudioBuffer {
	const ctx = audioContext();
	const buf = ctx.createBuffer(clip.channels.length, frames(clip), clip.sampleRate);
	for (const [i, c] of clip.channels.entries()) {
		buf.copyToChannel(c as Float32Array<ArrayBuffer>, i);
	}
	return buf;
}

export function formatTime(seconds: number): string {
	const m = Math.floor(seconds / 60);
	const s = seconds - m * 60;
	return `${m}:${s.toFixed(2).padStart(5, "0")}`;
}
