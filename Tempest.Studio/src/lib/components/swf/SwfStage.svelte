<!--
	The movie editor's canvas: a Ruffle preview of one character or timeline
	frame (`swf.preview`, pending edits applied), sized to the target's bounds
	at the current zoom so the overlay maps movie pixels exactly. The overlay
	draws each object's bounds; click selects (the smallest box under the
	pointer, Alt+click cycles through the others), drag moves, arrow keys
	nudge (Shift: 10 px). A new preview loads behind the old one and replaces
	it once Ruffle has it, so edits do not flash the canvas.
-->
<script lang="ts">
	import { inField } from "$lib/studio/keys";
	import { rpc } from "$lib/studio/rpc";
	import { loadRuffle, registerMovie, type RuffleElement } from "$lib/studio/ruffle";
	import { type SwfDoc } from "$lib/studio/swf.svelte";
	import type {
		SwfEntry,
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
	let host = $state<HTMLDivElement | null>(null);
	let vw = $state(0);
	let vh = $state(0);
	let rect = $state<SwfRect | null>(null);
	let status = $state<"loading" | "ready" | "error">("loading");
	let error = $state<string | null>(null);
	let missing = $state<string[]>([]);
	let current: { player: RuffleElement; route: { dispose(): void } } | null = null;
	let generation = 0;
	/** The target the visible preview shows (the overlay only fits that one). */
	let shown = $state("");
	const targetKey = $derived(JSON.stringify(target));

	const PAD = 24;
	const scale = $derived.by(() => {
		if (!rect) return 1;
		if (doc.zoom > 0) return doc.zoom;
		const fit = Math.min((vw - PAD * 2) / rect.w, (vh - PAD * 2) / rect.h);
		return Math.max(0.02, Math.min(fit, 4));
	});
	const width = $derived(rect ? Math.max(1, Math.round(rect.w * scale)) : 0);
	const height = $derived(rect ? Math.max(1, Math.round(rect.h * scale)) : 0);

	$effect(() => {
		const t = targetKey;
		void doc.version;
		if (!host) return;
		void show(JSON.parse(t) as SwfPreviewTarget, t);
	});

	$effect(() => {
		return () => {
			generation++;
			current?.route.dispose();
			current = null;
		};
	});

	async function show(t: SwfPreviewTarget, key: string): Promise<void> {
		const gen = ++generation;
		status = current && shown === key ? "ready" : "loading";
		error = null;
		try {
			const [preview, libraries, ruffle] = await Promise.all([
				rpc("swf.preview", {
					path: doc.path,
					index: doc.index,
					target: t,
					edits: doc.pending,
				}),
				doc.librariesOnce(),
				loadRuffle(),
			]);
			if (gen !== generation || !host) return;
			const route = registerMovie(preview.swf, libraries.libraries);
			const player = ruffle.newest().createPlayer();
			player.style.position = "absolute";
			player.style.inset = "0";
			player.style.width = "100%";
			player.style.height = "100%";
			player.style.opacity = "0";
			host.append(player);
			await player.ruffle().load({
				url: route.url,
				scale: "exactFit",
				letterbox: "off",
				wmode: t.kind === "root" ? "opaque" : "transparent",
				backgroundColor: t.kind === "root" ? null : "#00000000",
				contextMenu: "off",
			});
			if (gen !== generation) {
				player.remove();
				route.dispose();
				return;
			}
			rect = preview.rect;
			missing = preview.missing;
			shown = key;
			player.style.opacity = "1";
			const old = current;
			current = { player, route };
			if (old) {
				old.player.remove();
				old.route.dispose();
			}
			status = "ready";
		} catch (error_) {
			if (gen !== generation) return;
			error = error_ instanceof Error ? error_.message : String(error_);
			status = "error";
		}
	}

	// ----- overlay -------------------------------------------------------------

	const box = (r: SwfRect) => {
		const o = rect ?? { x: 0, y: 0, w: 0, h: 0 };
		return {
			left: (r.x - o.x) * scale,
			top: (r.y - o.y) * scale,
			width: Math.max(1, r.w * scale),
			height: Math.max(1, r.h * scale),
		};
	};

	const pickable = $derived(entries.filter((e) => e.rect && e.clipDepth === null));
	let drag = $state<{
		entry: SwfEntry;
		x0: number;
		y0: number;
		dx: number;
		dy: number;
		moved: boolean;
	} | null>(null);

	function toMovie(event: PointerEvent): { x: number; y: number } {
		const r = host!.getBoundingClientRect();
		const o = rect ?? { x: 0, y: 0, w: 0, h: 0 };
		return {
			x: (event.clientX - r.left) / scale + o.x,
			y: (event.clientY - r.top) / scale + o.y,
		};
	}

	function underPointer(p: { x: number; y: number }): SwfEntry[] {
		return pickable
			.filter((e) => {
				const r = e.rect!;
				return p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
			})
			.sort((a, b) => a.rect!.w * a.rect!.h - b.rect!.w * b.rect!.h);
	}

	function onPointerDown(event: PointerEvent) {
		if (event.button !== 0 || !host) return;
		viewport?.focus({ preventScroll: true });
		const p = toMovie(event);
		const hits = underPointer(p);
		if (hits.length === 0) {
			doc.depth = null;
			return;
		}
		let pick = hits[0];
		const sel = hits.findIndex((e) => e.depth === doc.depth);
		if (event.altKey && sel >= 0) pick = hits[(sel + 1) % hits.length];
		else if (sel >= 0 && !event.altKey) pick = hits[sel];
		doc.depth = pick.depth;
		drag = { entry: pick, x0: event.clientX, y0: event.clientY, dx: 0, dy: 0, moved: false };
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
	}

	function onPointerMove(event: PointerEvent) {
		if (!drag) return;
		let dx = (event.clientX - drag.x0) / scale;
		let dy = (event.clientY - drag.y0) / scale;
		if (event.shiftKey) {
			if (Math.abs(dx) > Math.abs(dy)) dy = 0;
			else dx = 0;
		}
		const moved =
			drag.moved || Math.abs(event.clientX - drag.x0) + Math.abs(event.clientY - drag.y0) > 3;
		drag = { ...drag, dx, dy, moved };
	}

	function onPointerUp() {
		if (drag?.moved && (drag.dx !== 0 || drag.dy !== 0)) {
			doc.moveEntry(drag.entry, drag.dx, drag.dy);
		}
		drag = null;
	}

	function onKeyDown(event: KeyboardEvent) {
		if (inField(event)) return;
		const e = doc.selectedEntry;
		const step = event.shiftKey ? 10 : 1;
		const moves: Record<string, [number, number]> = {
			ArrowLeft: [-step, 0],
			ArrowRight: [step, 0],
			ArrowUp: [0, -step],
			ArrowDown: [0, step],
		};
		if (event.key === "Escape") {
			doc.depth = null;
			return;
		}
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
		<div class="relative shrink-0" style="width: {width}px; height: {height}px">
			<div bind:this={host} class="absolute inset-0"></div>
			{#if status === "ready" && rect && shown === targetKey}
				<div
					class="absolute inset-0 cursor-default"
					role="presentation"
					onpointerdown={onPointerDown}
					onpointermove={onPointerMove}
					onpointerup={onPointerUp}
					onpointercancel={() => (drag = null)}
				>
					{#if outline}
						{@const b = box(outline)}
						<div
							class="border-warning/70 pointer-events-none absolute border border-dashed"
							style="left: {b.left}px; top: {b.top}px; width: {b.width}px; height: {b.height}px"
						>
							<span
								class="bg-warning/80 text-warning-content absolute -top-4 left-0 px-1 text-[9px]"
								>stage</span
							>
						</div>
					{/if}
					{#each pickable as e (e.depth)}
						{@const sel = e.depth === doc.depth}
						{#if doc.showBounds || sel}
							{@const b = box(e.rect!)}
							{@const dx = sel && drag ? drag.dx * scale : 0}
							{@const dy = sel && drag ? drag.dy * scale : 0}
							<div
								class="pointer-events-none absolute {sel
									? 'border-primary bg-primary/10 z-10 border-2'
									: doc.entryEdited(e)
										? 'border-warning/80 border'
										: 'border border-white/25'}"
								style="left: {b.left + dx}px; top: {b.top +
									dy}px; width: {b.width}px; height: {b.height}px"
							>
								{#if sel}
									<span
										class="bg-primary text-primary-content absolute -top-4 left-0 px-1 font-mono text-[9px] whitespace-nowrap"
										>{e.name ?? doc.label(e.character)} · {e.depth}{drag?.moved
											? ` · ${(e.matrix.tx + (drag?.dx ?? 0)).toFixed(1)}, ${(e.matrix.ty + (drag?.dy ?? 0)).toFixed(1)}`
											: ""}</span
									>
								{/if}
							</div>
						{/if}
					{/each}
				</div>
			{/if}
		</div>
	</div>
	{#if status === "loading"}
		<div
			class="pointer-events-none absolute inset-0 flex items-center justify-center gap-2 text-xs opacity-70"
		>
			<span class="loading loading-sm loading-spinner"></span> Rendering…
		</div>
	{:else if status === "error"}
		<div class="alert alert-error absolute inset-x-3 top-3 block text-xs">
			<p class="font-semibold">The preview could not be rendered</p>
			<p class="mt-1 font-mono break-words">{error}</p>
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
