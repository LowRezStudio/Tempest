<!--
	The curve editor of a Matinee curve (`matineeEd.curve`): one line per
	component (x/y/z or r/g/b/a), drawn with the engine's evaluation
	(`evalCurve`), keys as dots (drag to move in time and value), and the
	selected key's tangent handles (drag to set the slope: an auto key
	becomes CIM_CurveUser, a CIM_CurveBreak key moves one side only).
	Wheel zooms the values, F fits.
-->
<script lang="ts">
	import { map } from "$lib/studio/map.svelte";
	import { evalCurve, points } from "$lib/studio/matinee-math";
	import { matineeEd } from "$lib/studio/matinee.svelte";

	const COLORS = ["#f38ba8", "#a6e3a1", "#89b4fa", "#f9e2af"];

	const curve = $derived(
		matineeEd.curveOf(
			matineeEd.curve ? { track: matineeEd.curve.track, curve: matineeEd.curve.path } : null,
		),
	);
	const length = $derived(Math.max(map.matinee?.length ?? 1, 0.01));
	const pts = $derived(curve ? points(curve.keys) : []);
	const width = $derived(curve?.width ?? 1);
	const names = $derived(
		curve?.components?.length
			? curve.components
			: width === 1
				? ["value"]
				: ["x", "y", "z", "w"].slice(0, width),
	);
	let hiddenComp = $state<Record<number, boolean>>({});

	let svg = $state<SVGSVGElement | null>(null);
	let w = $state(600);
	let h = $state(160);
	$effect(() => {
		if (!svg) return;
		const ro = new ResizeObserver(() => {
			w = svg?.clientWidth ?? 600;
			h = svg?.clientHeight ?? 160;
		});
		ro.observe(svg);
		return () => ro.disconnect();
	});

	// The value range shown; refitted when another curve opens.
	let lo = $state(0);
	let hi = $state(1);
	function fit() {
		let a = Infinity;
		let b = -Infinity;
		for (let i = 0; i <= 100; i++) {
			const v = pts.length ? evalCurve(pts, (length * i) / 100) : [0];
			for (const [k, x] of v.entries()) {
				if (hiddenComp[k]) continue;
				a = Math.min(a, x);
				b = Math.max(b, x);
			}
		}
		if (!Number.isFinite(a)) [a, b] = [0, 1];
		const pad = Math.max((b - a) * 0.1, 0.5);
		lo = a - pad;
		hi = b + pad;
	}
	$effect(() => {
		void matineeEd.curve;
		void pts.length;
		fit();
	});

	const PAD = 8;
	const sx = (t: number) => PAD + (t / length) * (w - 2 * PAD);
	const sy = (v: number) => PAD + (1 - (v - lo) / (hi - lo || 1)) * (h - 2 * PAD);
	const tx = (x: number) => ((x - PAD) / (w - 2 * PAD)) * length;
	const vy = (y: number) => lo + (1 - (y - PAD) / (h - 2 * PAD)) * (hi - lo);

	const lines = $derived.by(() => {
		if (!pts.length) return [] as string[];
		const n = Math.min(400, Math.max(80, Math.round(w / 3)));
		const samples = Array.from({ length: n + 1 }, (_, i) => {
			const t = (length * i) / n;
			return [t, evalCurve(pts, t)] as const;
		});
		return Array.from({ length: width }, (_, k) =>
			samples
				.map(
					([t, v], i) =>
						`${i ? "L" : "M"}${sx(t).toFixed(1)},${sy(v[k] ?? 0).toFixed(1)}`,
				)
				.join(""),
		);
	});

	const sel = $derived(
		map.matineeKey &&
			matineeEd.curve &&
			map.matineeKey.track === matineeEd.curve.track &&
			map.matineeKey.curve === matineeEd.curve.path
			? map.matineeKey.key
			: null,
	);

	/** Live drag state: the key or handle being moved and where it is. */
	let drag = $state<{
		key: number;
		comp: number;
		t: number;
		v: number;
		handle?: "arrive" | "leave";
		slope?: number;
	} | null>(null);

	function local(e: PointerEvent) {
		const r = svg!.getBoundingClientRect();
		return { x: e.clientX - r.left, y: e.clientY - r.top };
	}

	function dragKey(e: PointerEvent, key: number, comp: number) {
		e.stopPropagation();
		if (!matineeEd.curve) return;
		const ref = { track: matineeEd.curve.track, curve: matineeEd.curve.path, key };
		matineeEd.selectKey(ref);
		const el = e.currentTarget as SVGElement;
		el.setPointerCapture(e.pointerId);
		const p = pts[key];
		const lowT = key > 0 ? pts[key - 1].time + 0.001 : 0;
		const highT = key < pts.length - 1 ? pts[key + 1].time - 0.001 : length;
		drag = { key, comp, t: p.time, v: p.out[comp] ?? 0 };
		let moved = false;
		const start = local(e);
		const move = (ev: PointerEvent) => {
			const q = local(ev);
			if (!moved && Math.hypot(q.x - start.x, q.y - start.y) < 3) return;
			moved = true;
			// Shift: value only; Ctrl: time only.
			const t = ev.shiftKey ? p.time : Math.min(highT, Math.max(lowT, tx(q.x)));
			const v = ev.ctrlKey ? (p.out[comp] ?? 0) : vy(q.y);
			drag = { key, comp, t, v };
			map.matineeTime = t;
		};
		const up = () => {
			el.removeEventListener("pointermove", move);
			el.removeEventListener("pointerup", up);
			const d = drag;
			drag = null;
			if (!moved || !d) return;
			const value = [...p.out];
			value[comp] = Math.round(d.v * 1000) / 1000;
			void matineeEd.setKey(ref, {
				time: Math.abs(d.t - p.time) > 1e-6 ? Math.round(d.t * 1000) / 1000 : undefined,
				value: value.length === 1 ? value[0] : value,
			});
		};
		el.addEventListener("pointermove", move);
		el.addEventListener("pointerup", up);
	}

	/** Handle length on screen, in pixels. */
	const HANDLE = 40;
	function handlePos(key: number, comp: number, side: "arrive" | "leave", slope?: number) {
		const p = pts[key];
		const s = slope ?? (side === "arrive" ? p.arrive[comp] : p.leave[comp]) ?? 0;
		// A unit vector on screen along (dt, s·dt).
		const dx = sx(1) - sx(0);
		const dy = sy(s) - sy(0);
		const len = Math.hypot(dx, dy) || 1;
		const dir = side === "arrive" ? -1 : 1;
		return {
			x: sx(p.time) + (dir * HANDLE * dx) / len,
			y: sy(p.out[comp] ?? 0) + (dir * HANDLE * dy) / len,
		};
	}

	function dragHandle(e: PointerEvent, key: number, comp: number, side: "arrive" | "leave") {
		e.stopPropagation();
		if (!matineeEd.curve) return;
		const ref = { track: matineeEd.curve.track, curve: matineeEd.curve.path, key };
		const el = e.currentTarget as SVGElement;
		el.setPointerCapture(e.pointerId);
		const p = pts[key];
		const move = (ev: PointerEvent) => {
			const q = local(ev);
			const dt = tx(q.x) - p.time;
			if (Math.abs(dt) < 1e-6 || (side === "arrive" ? dt > 0 : dt < 0)) return;
			drag = {
				key,
				comp,
				t: p.time,
				v: p.out[comp] ?? 0,
				handle: side,
				slope: (vy(q.y) - (p.out[comp] ?? 0)) / dt,
			};
		};
		const up = () => {
			el.removeEventListener("pointermove", move);
			el.removeEventListener("pointerup", up);
			const d = drag;
			drag = null;
			if (!d || d.slope === undefined) return;
			const slope = Math.round(d.slope * 1000) / 1000;
			const arrive = [...p.arrive];
			const leave = [...p.leave];
			// CurveBreak: each side on its own; otherwise both sides follow.
			if (p.mode === "CIM_CurveBreak") (side === "arrive" ? arrive : leave)[comp] = slope;
			else {
				arrive[comp] = slope;
				leave[comp] = slope;
			}
			void matineeEd.setKey(ref, {
				arrive: arrive.length === 1 ? arrive[0] : arrive,
				leave: leave.length === 1 ? leave[0] : leave,
			});
		};
		el.addEventListener("pointermove", move);
		el.addEventListener("pointerup", up);
	}

	function wheel(e: WheelEvent) {
		e.preventDefault();
		const f = e.deltaY > 0 ? 1.15 : 1 / 1.15;
		const c = vy(local(e as unknown as PointerEvent).y);
		lo = c + (lo - c) * f;
		hi = c + (hi - c) * f;
	}

	function onKey(e: KeyboardEvent) {
		if (e.key === "f" || e.key === "F") {
			e.preventDefault();
			fit();
		}
	}
	const zeroY = $derived(sy(0));
</script>

<div class="border-base-300 flex min-h-0 flex-1 flex-col border-t">
	<div class="flex h-6 items-center gap-2 px-2">
		<span class="font-semibold">{curve?.name ?? "Curve"}</span>
		{#each names as nm, k (k)}
			<label class="flex items-center gap-1" style="color:{COLORS[k]}">
				<input
					type="checkbox"
					class="checkbox checkbox-xs"
					checked={!hiddenComp[k]}
					onchange={() => (hiddenComp[k] = !hiddenComp[k])}
				/>{nm}
			</label>
		{/each}
		<div class="flex-1"></div>
		<span class="opacity-50"
			>drag keys (Shift: value only, Ctrl: time only) and handles · wheel zooms · F fits</span
		>
		<button class="btn btn-ghost btn-xs" onclick={fit}>Fit</button>
		<button class="btn btn-ghost btn-xs" onclick={() => (matineeEd.curve = null)}>Close</button>
	</div>
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
	<svg
		bind:this={svg}
		class="bg-base-300/40 min-h-0 w-full flex-1 outline-none"
		role="application"
		aria-label="Curve editor"
		tabindex="0"
		onwheel={wheel}
		onkeydown={onKey}
	>
		<line x1="0" x2={w} y1={zeroY} y2={zeroY} stroke="currentColor" stroke-opacity="0.2" />
		<line
			x1={sx(map.matineeTime)}
			x2={sx(map.matineeTime)}
			y1="0"
			y2={h}
			stroke="var(--color-warning, #f9e2af)"
			stroke-width="1"
		/>
		{#each lines as d, k (k)}
			{#if !hiddenComp[k]}<path {d} fill="none" stroke={COLORS[k]} stroke-width="1.5" />{/if}
		{/each}
		{#each pts as p, i (i)}
			{#each p.out as v, k (k)}
				{#if !hiddenComp[k]}
					{@const live = drag && drag.key === i && drag.comp === k && !drag.handle}
					{#if sel === i && p.mode !== "CIM_Linear" && p.mode !== "CIM_Constant"}
						{#each ["arrive", "leave"] as const as side (side)}
							{@const hp = handlePos(
								i,
								k,
								side,
								drag?.handle === side && drag.key === i && drag.comp === k
									? drag.slope
									: undefined,
							)}
							<line
								x1={sx(p.time)}
								y1={sy(v)}
								x2={hp.x}
								y2={hp.y}
								stroke={COLORS[k]}
								stroke-opacity="0.6"
							/>
							<circle
								cx={hp.x}
								cy={hp.y}
								r="4"
								fill="white"
								stroke={COLORS[k]}
								class="cursor-pointer"
								role="button"
								tabindex="-1"
								aria-label="{side} tangent"
								onpointerdown={(e) => dragHandle(e, i, k, side)}
							/>
						{/each}
					{/if}
					<circle
						cx={sx(live ? drag!.t : p.time)}
						cy={sy(live ? drag!.v : v)}
						r={sel === i ? 5 : 4}
						fill={sel === i ? "#f9e2af" : COLORS[k]}
						stroke="black"
						stroke-opacity="0.5"
						class="cursor-move"
						role="button"
						tabindex="-1"
						aria-label="key {i}"
						onpointerdown={(e) => dragKey(e, i, k)}
					>
						<title
							>{p.time.toFixed(3)} s: {v.toFixed(3)} ({p.mode.replace(
								"CIM_",
								"",
							)})</title
						>
					</circle>
				{/if}
			{/each}
		{/each}
	</svg>
</div>
