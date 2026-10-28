<!--
	The movie editor's canvas, stacked Ruffle layers (`SwfLayer`): the target
	(a timeline frame or one character) without its floating objects, then
	each floating object alone, placed with a CSS transform. An object floats
	once it is first edited (`SwfDoc.float`): its layer loads, then the frame
	re-renders without it, and from then on moving, scaling, rotating or
	fading it only changes that transform, so nothing re-renders. The
	visible area is fixed per target, so an edit never re-fits the view.

	Click selects the smallest object under the pointer (Alt+click cycles
	through the others), press and drag moves the selection, double-click or
	Enter edits inside a clip, Backspace goes up, arrows nudge (Shift: 10 px).
	The transform tool (Q; V goes back to move) adds handles: corners and
	edges scale (Shift keeps proportions, Alt scales from the centre), the
	knob rotates (Shift: 15° steps).
-->
<script lang="ts">
	import { untrack } from "svelte";
	import SwfLayer from "$lib/components/swf/SwfLayer.svelte";
	import { inField } from "$lib/studio/keys";
	import {
		apply,
		defaultView,
		invert,
		mul,
		prerenderView,
		rotation,
		scaling,
		STAGE_PAD as PAD,
		stageKey,
		stageRequest,
		stageScale,
		stageViewport,
		translate,
		type SwfDoc,
	} from "$lib/studio/swf.svelte";
	import type {
		SwfEdit,
		SwfEntry,
		SwfMatrix,
		SwfPreviewTarget,
		SwfRect,
	} from "../../../../electron/src/ipc/contract";

	interface Props {
		doc: SwfDoc;
		target: SwfPreviewTarget;
		/** Objects the overlay offers for selection (the current display list). */
		entries?: SwfEntry[];
		/** The movie's stage, in the target's coordinates (dashed outline). */
		outline?: SwfRect | null;
	}
	const { doc, target, entries = [], outline = null }: Props = $props();

	let viewport = $state<HTMLDivElement | null>(null);
	let vw = $state(0);
	let vh = $state(0);
	let error = $state<string | null>(null);
	let missing = $state<string[]>([]);
	/** Area the base layer showed last (character targets: their bounds). */
	let baseRect = $state<SwfRect | null>(null);
	let baseShown = $state("");
	/** The base layer is rendering a new picture (shown with a badge until it swaps in). */
	let baseBusy = $state(false);

	const targetKey = $derived(JSON.stringify(target));
	const timeline = $derived(target.kind !== "character");

	// ----- fixed view per target ---------------------------------------------

	/** Areas already chosen, per target, for this document. */
	const views = new Map<string, SwfRect>();
	const view = $derived.by((): SwfRect | null => {
		if (!timeline) return null;
		return views.get(targetKey) ?? defaultView(target, entries, outline);
	});
	$effect(() => {
		if (view && !views.has(targetKey)) views.set(targetKey, view);
	});
	const rect = $derived(view ?? baseRect);

	$effect(() => {
		stageViewport.w = vw;
		stageViewport.h = vh;
	});
	const scale = $derived(rect ? stageScale(rect, doc.zoom, { w: vw, h: vh }) : 1);
	const width = $derived(rect ? Math.max(1, Math.round(rect.w * scale)) : 0);
	const height = $derived(rect ? Math.max(1, Math.round(rect.h * scale)) : 0);

	// ----- floating layers ------------------------------------------------------

	const selected = $derived(timeline ? doc.selectedEntry : null);
	const sprite = $derived(doc.sprite);
	const localOf = (e: SwfEntry) => doc.chars.get(e.character)?.rect ?? null;
	const local = $derived(selected ? localOf(selected) : null);

	/** Objects with a layer of their own (see `SwfDoc.float`). */
	/** The display list minus the layers hidden in the editor. */
	const shown = $derived(entries.filter((e) => !doc.hidden.includes(e.depth)));

	const floats = $derived(
		timeline
			? shown.filter(
					(e) => doc.floated.includes(e.depth) && e.clipDepth === null && !!localOf(e),
				)
			: [],
	);
	/** Edits that change what an object draws (not where this timeline puts it). */
	const contentEdits = $derived(
		doc.pending.filter(
			(e: SwfEdit) =>
				!((e.kind === "place" || e.kind === "deletePlace") && e.sprite === sprite),
		),
	);
	const contentKey = $derived(JSON.stringify(contentEdits));
	const layerKey = (e: SwfEntry) => `${e.character}|${contentKey}|${doc.stamp}`;
	/** What each layer has rendered, by depth. */
	let loaded = $state<Record<number, { key: string; rect: SwfRect }>>({});
	let loadedFor = "";
	$effect(() => {
		if (loadedFor !== targetKey) {
			loadedFor = targetKey;
			loaded = {};
		}
	});
	/** Floating objects whose layer is on screen: the frame is drawn without them. */
	const ready = $derived(floats.filter((e) => loaded[e.depth]?.key === layerKey(e)));
	const readyTags = $derived(new Set(ready.flatMap((e) => doc.tagsOf(e))));

	/** A drag in progress: the matrix it would set. */
	let live = $state<{ depth: number; m: SwfMatrix } | null>(null);
	/** Set on release until the timeline reports it (keeps the layer from snapping back). */
	let committed = $state<Record<number, { m: SwfMatrix; from: string }>>({});
	function matrixOf(e: SwfEntry): SwfMatrix {
		if (live?.depth === e.depth) return live.m;
		const c = committed[e.depth];
		return c && c.from === JSON.stringify(e.matrix) ? c.m : e.matrix;
	}
	const matrix = $derived(selected ? matrixOf(selected) : null);

	// A selected clip is often entered next (Enter, double-click): render it ahead.
	$effect(() => {
		const id = selected?.character;
		if (id === undefined || doc.chars.get(id)?.kind !== "sprite") return;
		untrack(() => void prerenderView(doc, `char:${id}`).catch(() => {}));
	});

	// The floating objects' own placement edits only move their layers.
	const baseRequest = $derived(
		stageRequest(doc, target, view, { depths: ready.map((e) => e.depth), tags: readyTags }),
	);
	// A timeline target waits for its timeline: without it the view is
	// unknown, and the picture would be rendered twice.
	const baseKey = $derived(
		!timeline || doc.timelineReady ? stageKey(baseRequest, doc.stamp) : "",
	);

	function layerStyle(e: SwfEntry): string {
		const m = matrixOf(e);
		const L = loaded[e.depth]?.rect ?? localOf(e)!;
		const [ox, oy] = screen(m.tx, m.ty);
		const shown = loaded[e.depth]?.key === layerKey(e);
		const alpha = shown ? (e.cxform?.mul[3] ?? 1) : 0;
		return [
			`width: ${L.w * scale}px`,
			`height: ${L.h * scale}px`,
			"transform-origin: 0 0",
			`transform: matrix(${m.a}, ${m.b}, ${m.c}, ${m.d}, ${ox}, ${oy}) translate(${L.x * scale}px, ${L.y * scale}px)`,
			`opacity: ${alpha}`,
		].join("; ");
	}

	// ----- coordinates ----------------------------------------------------------

	/** Movie pixels → overlay pixels. */
	const screen = (x: number, y: number): [number, number] => {
		const o = rect ?? { x: 0, y: 0 };
		return [(x - o.x) * scale, (y - o.y) * scale];
	};

	function toMovie(event: PointerEvent | MouseEvent): [number, number] {
		const box = (event.currentTarget as HTMLElement)
			.closest("[data-stage]")!
			.getBoundingClientRect();
		const o = rect ?? { x: 0, y: 0 };
		return [(event.clientX - box.left) / scale + o.x, (event.clientY - box.top) / scale + o.y];
	}

	/** Corners of the selection in overlay pixels (TL, TR, BR, BL). */
	const corners = $derived.by((): [number, number][] => {
		if (!matrix || !local) return [];
		const L = local;
		return (
			[
				[L.x, L.y],
				[L.x + L.w, L.y],
				[L.x + L.w, L.y + L.h],
				[L.x, L.y + L.h],
			] as [number, number][]
		).map(([x, y]) => screen(...apply(matrix, x, y)));
	});

	// ----- picking --------------------------------------------------------------

	const pickable = $derived(shown.filter((e) => e.rect && e.clipDepth === null));

	function contains(e: SwfEntry, x: number, y: number): boolean {
		const L = e === selected ? local : null;
		if (L) {
			const inv = invert(matrixOf(e));
			if (inv) {
				const [lx, ly] = apply(inv, x, y);
				return lx >= L.x && lx <= L.x + L.w && ly >= L.y && ly <= L.y + L.h;
			}
		}
		const r = e.rect!;
		return x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h;
	}

	function under(x: number, y: number): SwfEntry[] {
		return pickable
			.filter((e) => contains(e, x, y))
			.sort((a, b) => a.rect!.w * a.rect!.h - b.rect!.w * b.rect!.h);
	}

	// ----- dragging ---------------------------------------------------------------

	type Drag =
		| {
				kind: "move";
				entry: SwfEntry;
				m0: SwfMatrix;
				p0: [number, number];
				x0: number;
				y0: number;
				moved: boolean;
				click: SwfEntry | null;
		  }
		| { kind: "scale"; entry: SwfEntry; m0: SwfMatrix; hx: number; hy: number; moved: boolean }
		| { kind: "rotate"; entry: SwfEntry; m0: SwfMatrix; p0: [number, number]; moved: boolean };
	let drag: Drag | null = null;

	function begin(event: PointerEvent, d: Drag) {
		drag = d;
		try {
			(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
		} catch {
			// Not an active pointer (synthetic events): the drag still works inside the stage.
		}
		event.stopPropagation();
		event.preventDefault();
	}

	/** Drop a drag without applying it. */
	function cancelDrag() {
		drag = null;
		live = null;
	}

	// A press the page never sees released (the window manager takes
	// Alt+click on Linux, the window loses focus) must not leave a drag
	// running: the selection would keep following the pointer.
	$effect(() => {
		window.addEventListener("blur", cancelDrag);
		return () => window.removeEventListener("blur", cancelDrag);
	});

	function onPointerDown(event: PointerEvent) {
		if (event.button !== 0) return;
		cancelDrag();
		viewport?.focus({ preventScroll: true });
		const [x, y] = toMovie(event);
		const hits = under(x, y);
		if (hits.length === 0) {
			doc.depth = null;
			return;
		}
		const sel = selected ? hits.indexOf(selected) : -1;
		let entry: SwfEntry;
		let click: SwfEntry | null = null;
		if (event.altKey && sel >= 0) {
			entry = hits[(sel + 1) % hits.length];
		} else if (sel >= 0) {
			// Pressing on the selection drags it; a plain click still picks the
			// smallest object under the pointer (a full-stage background must
			// not swallow every later click).
			entry = selected!;
			click = hits[0] !== selected ? hits[0] : null;
		} else {
			entry = hits[0];
		}
		doc.depth = entry.depth;
		begin(event, {
			kind: "move",
			entry,
			m0: matrixOf(entry),
			p0: [x, y],
			x0: event.clientX,
			y0: event.clientY,
			moved: false,
			click,
		});
	}

	function onHandleDown(event: PointerEvent, hx: number, hy: number) {
		if (event.button !== 0 || !selected || !matrix) return;
		doc.float(selected);
		begin(event, { kind: "scale", entry: selected, m0: matrix, hx, hy, moved: false });
	}

	function onKnobDown(event: PointerEvent) {
		if (event.button !== 0 || !selected || !matrix) return;
		doc.float(selected);
		begin(event, {
			kind: "rotate",
			entry: selected,
			m0: matrix,
			p0: toMovie(event),
			moved: false,
		});
	}

	function onPointerMove(event: PointerEvent) {
		const d = drag;
		if (!d) return;
		// The release happened where we could not see it.
		if (event.buttons === 0 && event.pointerType === "mouse" && event.isTrusted) {
			onPointerUp();
			return;
		}
		const [x, y] = toMovie(event);
		if (d.kind === "move") {
			const far = Math.abs(event.clientX - d.x0) + Math.abs(event.clientY - d.y0) > 3;
			if (!d.moved && !far) return;
			if (!d.moved) doc.float(d.entry);
			d.moved = true;
			let dx = x - d.p0[0];
			let dy = y - d.p0[1];
			if (event.shiftKey) {
				if (Math.abs(dx) > Math.abs(dy)) dy = 0;
				else dx = 0;
			}
			live = { depth: d.entry.depth, m: mul(translate(dx, dy), d.m0) };
		} else if (d.kind === "scale" && local) {
			const inv = invert(d.m0);
			if (!inv) return;
			d.moved = true;
			const [px, py] = apply(inv, x, y);
			const L = local;
			const hx = L.x + d.hx * L.w;
			const hy = L.y + d.hy * L.h;
			const cx = event.altKey ? L.x + L.w / 2 : L.x + (1 - d.hx) * L.w;
			const cy = event.altKey ? L.y + L.h / 2 : L.y + (1 - d.hy) * L.h;
			let sx = d.hx === 0.5 || hx === cx ? 1 : (px - cx) / (hx - cx);
			let sy = d.hy === 0.5 || hy === cy ? 1 : (py - cy) / (hy - cy);
			if (event.shiftKey) {
				const s =
					d.hx === 0.5 ? sy : d.hy === 0.5 ? sx : Math.abs(sx) > Math.abs(sy) ? sx : sy;
				sx = Math.sign(sx || 1) * Math.abs(s);
				sy = Math.sign(sy || 1) * Math.abs(s);
			}
			const clamp = (v: number) => (Math.abs(v) < 0.01 ? (v < 0 ? -0.01 : 0.01) : v);
			live = {
				depth: d.entry.depth,
				m: mul(
					d.m0,
					mul(translate(cx, cy), mul(scaling(clamp(sx), clamp(sy)), translate(-cx, -cy))),
				),
			};
		} else if (d.kind === "rotate" && local) {
			d.moved = true;
			const [cx, cy] = apply(d.m0, local.x + local.w / 2, local.y + local.h / 2);
			let r = Math.atan2(y - cy, x - cx) - Math.atan2(d.p0[1] - cy, d.p0[0] - cx);
			if (event.shiftKey) r = Math.round(r / (Math.PI / 12)) * (Math.PI / 12);
			live = {
				depth: d.entry.depth,
				m: mul(translate(cx, cy), mul(rotation(r), mul(translate(-cx, -cy), d.m0))),
			};
		}
	}

	function onPointerUp() {
		const d = drag;
		drag = null;
		if (!d) return;
		if (d.moved && live) {
			committed = {
				...committed,
				[d.entry.depth]: { m: live.m, from: JSON.stringify(d.entry.matrix) },
			};
			doc.setMatrix(d.entry, live.m);
		} else if (d.kind === "move" && d.click) {
			doc.depth = d.click.depth;
		}
		live = null;
	}

	function onDoubleClick(event: MouseEvent) {
		if (!selected) return;
		event.preventDefault();
		doc.enter(selected);
	}

	function onKeyDown(event: KeyboardEvent) {
		if (inField(event)) return;
		const e = selected;
		if (event.key === "Escape") {
			doc.depth = null;
			return;
		}
		if (event.key === "v" || event.key === "V") {
			doc.tool = "move";
			return;
		}
		if (event.key === "q" || event.key === "Q") {
			doc.tool = "transform";
			return;
		}
		if (event.key === "Enter" && e) {
			event.preventDefault();
			doc.enter(e);
			return;
		}
		if (event.key === "Backspace") {
			event.preventDefault();
			doc.up();
			return;
		}
		const step = event.shiftKey ? 10 : 1;
		const moves: Record<string, [number, number]> = {
			ArrowLeft: [-step, 0],
			ArrowRight: [step, 0],
			ArrowUp: [0, -step],
			ArrowDown: [0, step],
		};
		const m = moves[event.key];
		if (!m || !e) return;
		event.preventDefault();
		doc.moveEntry(e, m[0], m[1]);
	}

	function onWheel(event: WheelEvent) {
		if (!event.ctrlKey) return;
		event.preventDefault();
		const z = doc.zoom || scale;
		doc.zoom = Math.max(0.05, Math.min(16, z * (event.deltaY < 0 ? 1.25 : 0.8)));
	}

	const HANDLES: [number, number, string][] = [
		[0, 0, "nwse-resize"],
		[0.5, 0, "ns-resize"],
		[1, 0, "nesw-resize"],
		[1, 0.5, "ew-resize"],
		[1, 1, "nwse-resize"],
		[0.5, 1, "ns-resize"],
		[0, 1, "nesw-resize"],
		[0, 0.5, "ew-resize"],
	];
	const handlePoint = (hx: number, hy: number): [number, number] => {
		const L = local!;
		return screen(...apply(matrix!, L.x + hx * L.w, L.y + hy * L.h));
	};
	const knob = $derived.by((): { top: [number, number]; at: [number, number] } | null => {
		if (!matrix || !local) return null;
		const top = handlePoint(0.5, 0);
		const mid = handlePoint(0.5, 0.5);
		let dx = top[0] - mid[0];
		let dy = top[1] - mid[1];
		const n = Math.hypot(dx, dy) || 1;
		dx /= n;
		dy /= n;
		if (n < 1) {
			dx = 0;
			dy = -1;
		}
		return { top, at: [top[0] + dx * 22, top[1] + dy * 22] };
	});

	/** The base layer shows the current target (the overlay fits it). */
	const baseReady = $derived(baseShown === targetKey);
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
	bind:this={viewport}
	bind:clientWidth={vw}
	bind:clientHeight={vh}
	class="swf-checker relative min-h-0 flex-1 overflow-auto outline-none"
	tabindex="0"
	role="application"
	aria-label="Movie stage"
	onkeydown={onKeyDown}
	onwheel={onWheel}
>
	<div class="flex min-h-full min-w-full items-center justify-center" style="padding: {PAD}px">
		<div
			class="relative shrink-0"
			style="width: {width}px; height: {height}px"
			data-stage
			data-x={rect?.x}
			data-y={rect?.y}
			data-scale={scale}
		>
			<!-- Another clip or character starts from an empty layer: keeping the
				last one's picture until the new one is drawn showed the wrong
				thing for a moment. Frames and edits of one target swap in place. -->
			{#key `${target.kind}|${"id" in target ? target.id : 0}`}
				<SwfLayer
					{doc}
					request={baseRequest}
					key={baseKey}
					transparent={target.kind !== "root"}
					still={!(target.kind === "character" && target.play)}
					onload={(r) => {
						baseRect = r.rect;
						missing = r.missing;
						baseShown = JSON.stringify(JSON.parse(r.key).target);
						error = null;
					}}
					onerror={(m) => (error = m)}
					onbusy={(b) => (baseBusy = b)}
				/>
			{/key}
			{#each floats as e (e.depth)}
				<div class="pointer-events-none absolute top-0 left-0" style={layerStyle(e)}>
					<SwfLayer
						{doc}
						request={{
							target: { kind: "character", id: e.character },
							edits: contentEdits,
						}}
						key={layerKey(e)}
						onload={(r) =>
							(loaded = { ...loaded, [e.depth]: { key: r.key, rect: r.rect } })}
						onerror={(m) => (error = m)}
					/>
				</div>
			{/each}
			{#if rect && baseReady}
				<div
					class="absolute inset-0 cursor-default"
					role="presentation"
					onpointerdown={onPointerDown}
					onpointermove={onPointerMove}
					onpointerup={onPointerUp}
					onpointercancel={cancelDrag}
					onlostpointercapture={() => drag && onPointerUp()}
					ondblclick={onDoubleClick}
				>
					{#if outline && timeline}
						{@const [x, y] = screen(outline.x, outline.y)}
						<div
							class="border-warning/70 pointer-events-none absolute border border-dashed"
							style="left: {x}px; top: {y}px; width: {outline.w *
								scale}px; height: {outline.h * scale}px"
						>
							<span
								class="bg-warning/80 text-warning-content absolute -top-4 left-0 px-1 text-[9px]"
								>stage</span
							>
						</div>
					{/if}
					{#if doc.showBounds}
						{#each pickable as e (e.depth)}
							{#if e !== selected}
								{@const [x, y] = screen(e.rect!.x, e.rect!.y)}
								<div
									class="pointer-events-none absolute border {doc.entryEdited(e)
										? 'border-warning/80'
										: 'border-white/25'}"
									style="left: {x}px; top: {y}px; width: {Math.max(
										1,
										e.rect!.w * scale,
									)}px; height: {Math.max(1, e.rect!.h * scale)}px"
								></div>
							{/if}
						{/each}
					{/if}
					{#if selected}
						{#if corners.length === 4}
							<svg
								class="pointer-events-none absolute inset-0 overflow-visible"
								{width}
								{height}
							>
								<polygon
									points={corners.map((c) => c.join(",")).join(" ")}
									class="fill-primary/10 stroke-primary"
									stroke-width="1.5"
								/>
								{#if doc.tool === "transform" && knob}
									<line
										x1={knob.top[0]}
										y1={knob.top[1]}
										x2={knob.at[0]}
										y2={knob.at[1]}
										class="stroke-primary"
									/>
								{/if}
							</svg>
							<span
								class="bg-primary text-primary-content pointer-events-none absolute px-1 font-mono text-[9px] whitespace-nowrap"
								style="left: {Math.min(
									...corners.map((c) => c[0]),
								)}px; top: {Math.min(...corners.map((c) => c[1])) - 14}px"
								>{selected.name ?? doc.label(selected.character)} · {selected.depth}{live
									? ` · ${live.m.tx.toFixed(1)}, ${live.m.ty.toFixed(1)}`
									: ""}</span
							>
							{#if doc.tool === "transform"}
								{#each HANDLES as [hx, hy, cursor] (`${hx}-${hy}`)}
									{@const [x, y] = handlePoint(hx, hy)}
									<div
										class="bg-base-100 border-primary absolute z-20 size-2 -translate-x-1/2 -translate-y-1/2 border"
										style="left: {x}px; top: {y}px; cursor: {cursor}"
										role="presentation"
										onpointerdown={(ev) => onHandleDown(ev, hx, hy)}
										onpointermove={onPointerMove}
										onpointerup={onPointerUp}
									></div>
								{/each}
								{#if knob}
									<div
										class="bg-primary absolute z-20 size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full"
										style="left: {knob.at[0]}px; top: {knob
											.at[1]}px; cursor: grab"
										title="Rotate (Shift: 15° steps)"
										role="presentation"
										onpointerdown={onKnobDown}
										onpointermove={onPointerMove}
										onpointerup={onPointerUp}
									></div>
								{/if}
							{/if}
						{:else if selected.rect}
							{@const [x, y] = screen(selected.rect.x, selected.rect.y)}
							<div
								class="border-primary bg-primary/10 pointer-events-none absolute border-2"
								style="left: {x}px; top: {y}px; width: {selected.rect.w *
									scale}px; height: {selected.rect.h * scale}px"
							></div>
						{/if}
					{/if}
				</div>
			{/if}
		</div>
	</div>
	{#if !baseReady && !error}
		<div
			class="pointer-events-none absolute inset-0 flex items-center justify-center gap-2 text-xs opacity-70"
		>
			<span class="loading loading-sm loading-spinner"></span> Rendering…
		</div>
	{/if}
	{#if error}
		<div class="alert alert-error absolute inset-x-3 top-3 block text-xs">
			<p class="font-semibold">The preview could not be rendered</p>
			<p class="mt-1 font-mono break-words">{error}</p>
		</div>
	{/if}
	{#if baseBusy && baseReady}
		<div
			class="bg-base-100/90 pointer-events-none absolute top-2 right-2 flex items-center gap-1 rounded px-2 py-0.5 text-[10px] shadow"
		>
			<span class="loading loading-xs loading-spinner"></span> updating view…
		</div>
	{/if}
	{#if missing.length > 0}
		<div
			class="bg-warning/90 text-warning-content sticky bottom-0 truncate px-2 py-0.5 font-mono text-[10px]"
			title={missing.join("\n")}
		>
			{missing.length} image{missing.length === 1 ? "" : "s"} not found: {missing[0]}
		</div>
	{/if}
</div>

<style>
	.swf-checker {
		background-color: var(--color-base-300);
		background-image:
			linear-gradient(45deg, rgb(127 127 127 / 0.12) 25%, transparent 25%),
			linear-gradient(-45deg, rgb(127 127 127 / 0.12) 25%, transparent 25%),
			linear-gradient(45deg, transparent 75%, rgb(127 127 127 / 0.12) 75%),
			linear-gradient(-45deg, transparent 75%, rgb(127 127 127 / 0.12) 75%);
		background-size: 16px 16px;
		background-position:
			0 0,
			0 8px,
			8px -8px,
			-8px 0;
	}
</style>
