<script lang="ts">
	import { Check, RotateCcw } from "@lucide/svelte";
	import { isCurve, type Dist } from "$lib/studio/particles";

	interface Props {
		/** The distribution as stored. */
		dist: Dist;
		/** Called with every change (for the live preview). */
		onchange: (d: Dist) => void;
		/** Called to persist. */
		onapply: (d: Dist) => Promise<void>;
	}
	const { dist, onchange, onapply }: Props = $props();

	// svelte-ignore state_referenced_locally
	let draft = $state<Dist>(structuredClone($state.snapshot(dist)));
	let canvas = $state<HTMLCanvasElement | null>(null);
	let dragging: { entry: number; k: number } | null = null;
	let saving = $state(false);
	// svelte-ignore state_referenced_locally
	let points = $state(dist.entries.length);

	const COLORS = ["#f38ba8", "#a6e3a1", "#89b4fa"];
	const dirty = $derived(JSON.stringify(draft) !== JSON.stringify(dist));
	/** Number of values per entry: comps × (1 or 2). */
	const values = $derived(draft.numElements >= 2 ? 2 : 1);
	const constant = $derived(!isCurve(draft));

	$effect(() => {
		// Reset when another distribution is shown.
		draft = structuredClone($state.snapshot(dist));
		points = dist.entries.length;
	});

	function range(): [number, number] {
		let lo = Number.POSITIVE_INFINITY;
		let hi = Number.NEGATIVE_INFINITY;
		for (const e of draft.entries) {
			for (const v of e) {
				lo = Math.min(lo, v);
				hi = Math.max(hi, v);
			}
		}
		if (!Number.isFinite(lo)) return [0, 1];
		if (hi - lo < 1e-6) {
			const pad = Math.max(1, Math.abs(hi) * 0.5);
			return [lo - pad, hi + pad];
		}
		const pad = (hi - lo) * 0.15;
		return [lo - pad, hi + pad];
	}

	function draw() {
		const c = canvas;
		if (!c) return;
		const w = c.clientWidth;
		const h = c.clientHeight;
		if (w === 0 || h === 0) return;
		if (c.width !== w * devicePixelRatio) {
			c.width = w * devicePixelRatio;
			c.height = h * devicePixelRatio;
		}
		const ctx = c.getContext("2d");
		if (!ctx) return;
		ctx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
		ctx.clearRect(0, 0, w, h);
		const n = draft.entries.length;
		const [lo, hi] = range();
		const px = (i: number) => 24 + (w - 32) * (n > 1 ? i / (n - 1) : 0.5);
		const py = (v: number) => h - 14 - ((h - 24) * (v - lo)) / (hi - lo);
		ctx.strokeStyle = "rgba(128,128,128,0.25)";
		ctx.lineWidth = 1;
		for (const v of [lo, (lo + hi) / 2, hi]) {
			ctx.beginPath();
			ctx.moveTo(24, py(v));
			ctx.lineTo(w - 8, py(v));
			ctx.stroke();
		}
		if (lo < 0 && hi > 0) {
			ctx.strokeStyle = "rgba(128,128,128,0.6)";
			ctx.beginPath();
			ctx.moveTo(24, py(0));
			ctx.lineTo(w - 8, py(0));
			ctx.stroke();
		}
		ctx.fillStyle = "rgba(160,160,160,0.8)";
		ctx.font = "9px ui-monospace, monospace";
		ctx.fillText(fmt(hi), 2, 12);
		ctx.fillText(fmt(lo), 2, h - 14);
		for (let v = 0; v < values; v++) {
			for (let comp = 0; comp < draft.comps; comp++) {
				const k = v * draft.comps + comp;
				ctx.strokeStyle = COLORS[comp % 3];
				ctx.setLineDash(v === 1 ? [3, 3] : []);
				ctx.lineWidth = 1.5;
				ctx.beginPath();
				for (let i = 0; i < n; i++) {
					const x = px(i);
					const y = py(draft.entries[i][k]);
					if (i === 0) ctx.moveTo(x, y);
					else ctx.lineTo(x, y);
				}
				ctx.stroke();
				ctx.setLineDash([]);
				ctx.fillStyle = COLORS[comp % 3];
				for (let i = 0; i < n; i++) {
					ctx.beginPath();
					ctx.arc(px(i), py(draft.entries[i][k]), 3, 0, Math.PI * 2);
					ctx.fill();
				}
			}
		}
	}

	$effect(() => {
		void draft;
		void canvas;
		draw();
	});

	function fmt(v: number): string {
		return Math.abs(v) >= 100 ? v.toFixed(0) : Math.abs(v) >= 1 ? v.toFixed(2) : v.toFixed(3);
	}

	function hit(event: MouseEvent): { entry: number; k: number } | null {
		const c = canvas;
		if (!c) return null;
		const r = c.getBoundingClientRect();
		const x = event.clientX - r.left;
		const y = event.clientY - r.top;
		const w = r.width;
		const h = r.height;
		const n = draft.entries.length;
		const [lo, hi] = range();
		let best: { entry: number; k: number; d: number } | null = null;
		for (let i = 0; i < n; i++) {
			const px = 24 + (w - 32) * (n > 1 ? i / (n - 1) : 0.5);
			for (let k = 0; k < draft.chunk; k++) {
				const py = h - 14 - ((h - 24) * (draft.entries[i][k] - lo)) / (hi - lo);
				const d = Math.hypot(px - x, py - y);
				if (d < 8 && (!best || d < best.d)) best = { entry: i, k, d };
			}
		}
		return best;
	}

	function down(event: MouseEvent) {
		dragging = hit(event);
	}

	function move(event: MouseEvent) {
		const c = canvas;
		if (!dragging || !c) return;
		const r = c.getBoundingClientRect();
		const h = r.height;
		const [lo, hi] = range();
		const v = lo + ((h - 14 - (event.clientY - r.top)) / (h - 24)) * (hi - lo);
		const entries = draft.entries.map((e) => [...e]);
		entries[dragging.entry][dragging.k] = Math.round(v * 1000) / 1000;
		draft = { ...draft, entries };
		onchange(draft);
	}

	function up() {
		dragging = null;
	}

	/** Change the number of entries, keeping the curve's shape. */
	function resample(wanted: number) {
		const count = Math.max(2, Math.min(256, Math.round(wanted)));
		const n = draft.entries.length;
		const entries: number[][] = [];
		for (let i = 0; i < count; i++) {
			const t = n > 1 ? (i / (count - 1)) * (n - 1) : 0;
			const a = Math.floor(t);
			const f = t - a;
			const e1 =
				draft.entries[Math.min(n - 1, a)] ?? Array.from({ length: draft.chunk }, () => 0);
			const e2 = draft.entries[Math.min(n - 1, a + 1)] ?? e1;
			entries.push(e1.map((v, k) => v + (e2[k] - v) * f));
		}
		// Keep the same time span: the last entry stays where it was.
		const span = n > 1 && draft.timeScale ? (n - 1) / draft.timeScale : 1;
		draft = { ...draft, entries, timeScale: (count - 1) / span };
		points = count;
		onchange(draft);
	}

	function setCell(i: number, k: number, text: string) {
		const v = Number.parseFloat(text);
		if (!Number.isFinite(v)) return;
		const entries = draft.entries.map((e) => [...e]);
		entries[i][k] = v;
		draft = { ...draft, entries };
		onchange(draft);
	}

	function setField(name: "timeScale" | "startTime", text: string) {
		const v = Number.parseFloat(text);
		if (!Number.isFinite(v)) return;
		draft = { ...draft, [name]: v };
		onchange(draft);
	}

	function revert() {
		draft = structuredClone($state.snapshot(dist));
		points = dist.entries.length;
		onchange(draft);
	}

	async function apply() {
		saving = true;
		try {
			await onapply(draft);
		} finally {
			saving = false;
		}
	}

	const span = $derived(
		draft.entries.length > 1 && draft.timeScale
			? (draft.entries.length - 1) / draft.timeScale
			: 0,
	);
	const label = (k: number) => {
		const comp = draft.comps === 3 ? ["X", "Y", "Z"][k % 3] : "";
		return draft.numElements >= 2 ? `${k < draft.comps ? "A" : "B"}${comp}` : comp || "value";
	};
</script>

<div class="border-base-300 bg-base-200/50 rounded border p-2 text-[11px]">
	<div class="mb-1 flex items-center gap-2">
		<span class="font-mono font-semibold">{draft.prop}</span>
		<span class="opacity-60"
			>{draft.kind}{draft.op === 1
				? " · random between A and B"
				: draft.op === 2
					? " · A or B"
					: ""}{constant
				? " · constant"
				: ` · ${draft.entries.length} points over ${span.toFixed(2)} s from ${draft.startTime}`}</span
		>
		<div class="flex-1"></div>
		{#if dirty}
			<button class="btn btn-ghost btn-xs" onclick={revert} title="Back to the stored values"
				><RotateCcw size={11} /></button
			>
			<button class="btn btn-primary btn-xs" onclick={apply} disabled={saving}>
				{#if saving}<span class="loading loading-xs loading-spinner"></span>{:else}<Check
						size={11}
					/>{/if}
				Apply
			</button>
		{/if}
	</div>
	<canvas
		bind:this={canvas}
		class="bg-base-300 h-28 w-full cursor-crosshair rounded"
		onmousedown={down}
		onmousemove={move}
		onmouseup={up}
		onmouseleave={up}
	></canvas>
	<div class="mt-1 flex flex-wrap items-center gap-2">
		<label class="flex items-center gap-1">
			points
			<input
				type="number"
				class="input input-xs w-14"
				min="2"
				max="256"
				value={points}
				onchange={(e) => resample(Number((e.currentTarget as HTMLInputElement).value))}
			/>
		</label>
		<label class="flex items-center gap-1" title="Entries per second (points − 1 ÷ duration)">
			time scale
			<input
				type="number"
				class="input input-xs w-16"
				step="0.1"
				value={draft.timeScale}
				onchange={(e) => setField("timeScale", (e.currentTarget as HTMLInputElement).value)}
			/>
		</label>
		<label class="flex items-center gap-1">
			start
			<input
				type="number"
				class="input input-xs w-16"
				step="0.05"
				value={draft.startTime}
				onchange={(e) => setField("startTime", (e.currentTarget as HTMLInputElement).value)}
			/>
		</label>
		<span class="opacity-50"
			>drag points; 1 = 360° for rotations; sizes and positions in Unreal units</span
		>
	</div>
	<div class="mt-1 max-h-32 overflow-auto">
		<table class="table-xs table font-mono">
			<thead
				><tr
					><th>#</th>{#each { length: draft.chunk } as _, k (k)}<th
							style="color:{COLORS[(k % draft.comps) % 3]}">{label(k)}</th
						>{/each}</tr
				></thead
			>
			<tbody>
				{#each draft.entries as e, i (i)}
					<tr>
						<td class="opacity-50">{i}</td>
						{#each e as v, k (k)}
							<td
								><input
									class="input input-xs w-20"
									value={v}
									onchange={(ev) =>
										setCell(i, k, (ev.currentTarget as HTMLInputElement).value)}
								/></td
							>
						{/each}
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>
