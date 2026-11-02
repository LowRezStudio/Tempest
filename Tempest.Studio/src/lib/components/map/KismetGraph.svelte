<!--
	A Kismet sequence as a node graph. Drag the background to pan, the wheel
	to zoom, a node's title to move it (kept in the Studio: cooked packages
	have no node positions). Drag from an output to an input, or from a
	variable connector to a variable, to link them; Alt+click a link (or
	select it and press Delete) to break it. Double-click a sub-sequence to
	go into it.
-->
<script lang="ts">
	import {
		inputPoint,
		layout,
		outputPoint,
		variablePoint,
		type Box,
	} from "$lib/studio/kismet-layout";
	import { map } from "$lib/studio/map.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { KismetNode } from "../../../../electron/src/ipc/contract";

	let host = $state<HTMLDivElement | null>(null);
	let pan = $state({ x: 40, y: 40 });
	let zoom = $state(1);
	/** A connection being dragged: from a connector to the pointer. */
	let pending = $state<{
		from: number;
		kind: "output" | "variable";
		index: number;
		x: number;
		y: number;
		px: number;
		py: number;
	} | null>(null);
	let selectedLink = $state<string | null>(null);
	let fittedFor: string | null = null;

	const nodes = $derived(map.graph?.nodes ?? []);
	const byId = $derived(new Map(nodes.map((n) => [n.export, n])));
	const boxes = $derived(layout(nodes, map.layout[map.layoutKey()] ?? {}));
	const frames = $derived(nodes.filter((n) => n.kind === "frame"));

	type Link = { id: string; d: string; kind: "exec" | "var"; remove: () => void };
	const links = $derived.by(() => {
		const out: Link[] = [];
		for (const n of nodes) {
			const b = boxes.get(n.export);
			if (!b) continue;
			for (const [k, o] of n.outputs.entries()) {
				const a = outputPoint(b, k);
				for (const l of o.links) {
					const tb = l.op === null ? undefined : boxes.get(l.op);
					if (!tb || l.op === null) continue;
					const t = inputPoint(tb, l.input);
					const dx = Math.max(40, Math.abs(t.x - a.x) / 2);
					const op = l.op;
					out.push({
						id: `o${n.export}:${k}>${op}:${l.input}`,
						d: `M${a.x},${a.y} C${a.x + dx},${a.y} ${t.x - dx},${t.y} ${t.x},${t.y}`,
						kind: "exec",
						remove: () =>
							void map.link({
								from: n.export,
								output: k,
								to: op,
								input: l.input,
								remove: true,
							}),
					});
				}
			}
			const count = n.variables.length + n.events.length;
			for (const [k, v] of n.variables.entries()) {
				const a = variablePoint(b, k, count);
				for (const id of v.linked) {
					const tb = id === null ? undefined : boxes.get(id);
					if (!tb || id === null) continue;
					const t = { x: tb.x + tb.w / 2, y: tb.y };
					out.push({
						id: `v${n.export}:${k}>${id}`,
						d: `M${a.x},${a.y} C${a.x},${a.y + 40} ${t.x},${t.y - 40} ${t.x},${t.y}`,
						kind: "var",
						remove: () =>
							void map.link({ from: n.export, variable: k, to: id, remove: true }),
					});
				}
			}
		}
		return out;
	});

	function fit() {
		if (!host || boxes.size === 0) return;
		let x0 = Infinity,
			y0 = Infinity,
			x1 = -Infinity,
			y1 = -Infinity;
		for (const b of boxes.values()) {
			x0 = Math.min(x0, b.x);
			y0 = Math.min(y0, b.y);
			x1 = Math.max(x1, b.x + b.w);
			y1 = Math.max(y1, b.y + b.h);
		}
		const w = host.clientWidth;
		const h = host.clientHeight;
		zoom = Math.min(1.2, Math.max(0.15, Math.min(w / (x1 - x0 + 120), h / (y1 - y0 + 120))));
		pan = {
			x: (w - (x1 - x0) * zoom) / 2 - x0 * zoom,
			y: (h - (y1 - y0) * zoom) / 2 - y0 * zoom,
		};
	}

	$effect(() => {
		const key = map.layoutKey();
		if (nodes.length > 0 && fittedFor !== key) {
			fittedFor = key;
			requestAnimationFrame(fit);
		}
	});

	function toCanvas(e: { clientX: number; clientY: number }) {
		const r = host!.getBoundingClientRect();
		return { x: (e.clientX - r.left - pan.x) / zoom, y: (e.clientY - r.top - pan.y) / zoom };
	}

	function onWheel(e: WheelEvent) {
		e.preventDefault();
		if (!host) return;
		const r = host.getBoundingClientRect();
		const mx = e.clientX - r.left;
		const my = e.clientY - r.top;
		const next = Math.min(3, Math.max(0.1, zoom * Math.exp(-e.deltaY * 0.0015)));
		pan = { x: mx - ((mx - pan.x) * next) / zoom, y: my - ((my - pan.y) * next) / zoom };
		zoom = next;
	}

	function onBackgroundDown(e: PointerEvent) {
		if (e.button !== 0 && e.button !== 1) return;
		if ((e.target as HTMLElement).closest("[data-node],[data-link]")) return;
		map.kismetSelected = null;
		selectedLink = null;
		const start = { x: e.clientX, y: e.clientY, px: pan.x, py: pan.y };
		const el = e.currentTarget as HTMLElement;
		el.setPointerCapture(e.pointerId);
		const move = (ev: PointerEvent) => {
			pan = { x: start.px + ev.clientX - start.x, y: start.py + ev.clientY - start.y };
		};
		const up = () => {
			el.removeEventListener("pointermove", move);
			el.removeEventListener("pointerup", up);
		};
		el.addEventListener("pointermove", move);
		el.addEventListener("pointerup", up);
	}

	function onNodeDown(e: PointerEvent, n: KismetNode, b: Box) {
		if (e.button !== 0) return;
		e.stopPropagation();
		map.kismetSelected = n.export;
		selectedLink = null;
		const start = toCanvas(e);
		const el = e.currentTarget as HTMLElement;
		el.setPointerCapture(e.pointerId);
		let moved = false;
		const move = (ev: PointerEvent) => {
			const p = toCanvas(ev);
			if (!moved && Math.hypot(p.x - start.x, p.y - start.y) < 3) return;
			moved = true;
			map.moveNode(
				n.export,
				Math.round(b.x + p.x - start.x),
				Math.round(b.y + p.y - start.y),
			);
		};
		const up = () => {
			el.removeEventListener("pointermove", move);
			el.removeEventListener("pointerup", up);
		};
		el.addEventListener("pointermove", move);
		el.addEventListener("pointerup", up);
	}

	function startLink(
		e: PointerEvent,
		n: KismetNode,
		kind: "output" | "variable",
		index: number,
		at: { x: number; y: number },
	) {
		e.stopPropagation();
		const el = host!;
		const p = toCanvas(e);
		pending = { from: n.export, kind, index, x: at.x, y: at.y, px: p.x, py: p.y };
		const move = (ev: PointerEvent) => {
			const q = toCanvas(ev);
			if (pending) pending = { ...pending, px: q.x, py: q.y };
		};
		const up = (ev: PointerEvent) => {
			window.removeEventListener("pointermove", move);
			window.removeEventListener("pointerup", up);
			const target = (
				document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null
			)?.closest("[data-input],[data-node]") as HTMLElement | null;
			const p0 = pending;
			pending = null;
			if (!p0 || !target) return;
			const to = Number(
				target.dataset.node ?? target.closest("[data-node]")?.getAttribute("data-node"),
			);
			if (!Number.isFinite(to)) return;
			if (p0.kind === "output" && target.dataset.input !== undefined) {
				void map.link({
					from: p0.from,
					output: p0.index,
					to,
					input: Number(target.dataset.input),
				});
			} else if (p0.kind === "variable" && byId.get(to)?.kind === "variable") {
				void map.link({ from: p0.from, variable: p0.index, to });
			}
		};
		window.addEventListener("pointermove", move);
		window.addEventListener("pointerup", up);
		void el;
	}

	// ----- adding nodes -------------------------------------------------------
	/** The palette: where it shows (pixels in the view) and where the node goes (canvas). */
	let adding = $state<{ left: number; top: number; x: number; y: number } | null>(null);
	let paletteFilter = $state("");
	const KINDS = ["event", "action", "condition", "variable", "frame", "other"];
	const shownPalette = $derived.by(() => {
		const q = paletteFilter.trim().toLowerCase();
		return (map.palette ?? [])
			.filter((c) => c.kind !== "sequence" && (!q || c.class.includes(q)))
			.sort((a, b) => KINDS.indexOf(a.kind) - KINDS.indexOf(b.kind) || b.count - a.count);
	});

	/** Open the palette at a pointer position (or the middle of the view). */
	export function openPalette(e?: { clientX: number; clientY: number }) {
		if (!host) return;
		const r = host.getBoundingClientRect();
		const cx = e ? e.clientX : r.left + r.width / 2;
		const cy = e ? e.clientY : r.top + r.height / 3;
		const p = toCanvas({ clientX: cx, clientY: cy });
		adding = {
			left: Math.min(cx - r.left, r.width - 330),
			top: Math.min(cy - r.top, r.height - 340),
			x: p.x,
			y: p.y,
		};
		paletteFilter = "";
		void map.loadPalette();
	}

	async function pick(c: { kind: string; example: { path: string; index: number } }) {
		const at = adding;
		adding = null;
		if (at) await map.addNode(c.example, c.kind, at.x, at.y);
	}

	async function deleteSelected() {
		const node = map.kismetSelected;
		const n = node === null ? undefined : byId.get(node);
		if (!n) return;
		const ok = await ui.confirm({
			title: `Delete ${n.title || n.name}?`,
			message:
				"The node and every link to it are removed from the sequence (Ctrl+Z brings it back).",
			confirm: "Delete",
			danger: true,
		});
		if (ok === true) await map.removeNode(n.export);
	}

	function onKey(e: KeyboardEvent) {
		const t = e.target as HTMLElement | null;
		if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;
		if (e.key === "Escape" && adding) {
			adding = null;
		} else if ((e.key === "Delete" || e.key === "Backspace") && selectedLink) {
			links.find((l) => l.id === selectedLink)?.remove();
			selectedLink = null;
			e.preventDefault();
		} else if ((e.key === "Delete" || e.key === "Backspace") && map.kismetSelected !== null) {
			e.preventDefault();
			void deleteSelected();
		} else if (e.key === "a" || e.key === "A") {
			openPalette();
			e.preventDefault();
		} else if (e.key === "f" || e.key === "F") {
			fit();
		}
	}

	const KIND_STYLE: Record<string, string> = {
		event: "border-error/70 bg-error/10",
		action: "border-info/60 bg-base-200",
		condition: "border-warning/70 bg-warning/10",
		sequence: "border-secondary/70 bg-secondary/10",
		other: "border-base-content/30 bg-base-200",
	};
	const title = (n: KismetNode) =>
		n.title || n.class.replace(/^(tg)?seq(act|event|cond|var)_/i, "").replaceAll("_", " ");
</script>

<svelte:window onkeydown={onKey} />

<div class="relative flex min-h-0 flex-1 flex-col">
	{#if frames.length > 0}
		<div class="border-base-300 bg-base-200 flex flex-wrap gap-1 border-b px-2 py-1 text-xs">
			<span class="opacity-60">Comments:</span>
			{#each frames as f (f.export)}
				<span class="badge badge-ghost badge-sm">{f.comment || f.name}</span>
			{/each}
		</div>
	{/if}
	<div
		class="bg-base-300/40 relative min-h-0 flex-1 cursor-grab overflow-hidden select-none"
		bind:this={host}
		onwheel={onWheel}
		onpointerdown={(e) => {
			if (adding && !(e.target as HTMLElement).closest("[data-palette]")) adding = null;
			onBackgroundDown(e);
		}}
		oncontextmenu={(e) => {
			e.preventDefault();
			if (!(e.target as HTMLElement).closest("[data-node]")) openPalette(e);
		}}
		role="application"
		aria-label="Kismet graph"
		style="background-image: radial-gradient(circle, rgb(127 132 156 / 0.25) 1px, transparent 1px); background-size: {24 *
			zoom}px {24 * zoom}px; background-position: {pan.x}px {pan.y}px"
	>
		<div
			class="absolute top-0 left-0 origin-top-left"
			style="transform: translate({pan.x}px, {pan.y}px) scale({zoom})"
		>
			<svg
				class="pointer-events-none absolute overflow-visible"
				style="left:0;top:0"
				width="1"
				height="1"
			>
				{#each links as l (l.id)}
					<path
						d={l.d}
						fill="none"
						stroke="transparent"
						stroke-width="10"
						class="pointer-events-auto cursor-pointer"
						data-link={l.id}
						role="button"
						tabindex="-1"
						onpointerdown={(e) => {
							e.stopPropagation();
							if (e.altKey) l.remove();
							else selectedLink = l.id;
						}}
					/>
					<path
						d={l.d}
						fill="none"
						stroke-width={selectedLink === l.id ? 3 : 1.6}
						class={selectedLink === l.id
							? "stroke-warning"
							: l.kind === "exec"
								? "stroke-base-content/70"
								: "stroke-error/60"}
						stroke-dasharray={l.kind === "var" ? "4 3" : undefined}
					/>
				{/each}
				{#if pending}
					<path
						d="M{pending.x},{pending.y} L{pending.px},{pending.py}"
						class="stroke-primary"
						stroke-width="2"
						fill="none"
					/>
				{/if}
			</svg>
			{#each nodes as n (n.export)}
				{@const b = boxes.get(n.export)}
				{#if b && n.kind === "variable"}
					<div
						class="absolute flex flex-col items-center justify-center rounded-full border-2 text-center text-[10px] leading-tight {map.kismetSelected ===
						n.export
							? 'border-warning'
							: 'border-error/60'} bg-base-100 cursor-pointer"
						style="left:{b.x}px;top:{b.y}px;width:{b.w}px;height:{b.h}px"
						data-node={n.export}
						role="button"
						tabindex="-1"
						title="{n.class} {n.name}"
						onpointerdown={(e) => onNodeDown(e, n, b)}
					>
						<span class="w-full truncate px-1 font-mono">{n.value ?? "?"}</span>
						<span class="opacity-50">{n.class.replace(/^seqvar_/i, "")}</span>
					</div>
				{:else if b}
					<div
						class="absolute rounded border-2 text-[11px] shadow {KIND_STYLE[n.kind] ??
							KIND_STYLE.other} {map.kismetSelected === n.export
							? 'ring-warning ring-2'
							: ''}"
						style="left:{b.x}px;top:{b.y}px;width:{b.w}px;height:{b.h}px"
						data-node={n.export}
						role="button"
						tabindex="-1"
						ondblclick={() =>
							n.kind === "sequence" && map.openKismet(map.kismetLevel!, n.export)}
					>
						<div
							class="bg-base-content/10 flex h-[22px] cursor-move items-center gap-1 truncate px-1.5 font-semibold"
							role="presentation"
							onpointerdown={(e) => onNodeDown(e, n, b)}
							title="{n.class} ({n.name}){n.originator
								? `\nfired by ${n.originator.path}`
								: ''}{n.comment ? `\n${n.comment}` : ''}"
						>
							<span class="truncate">{title(n)}</span>
							{#if n.kind === "sequence"}<span class="ml-auto opacity-50"
									>{n.subObjects} ›</span
								>{/if}
						</div>
						{#each n.inputs as c, k (k)}
							{@const p = inputPoint(b, k)}
							<div
								class="absolute flex items-center gap-1 {c.hidden
									? 'opacity-40'
									: ''}"
								style="left:-5px;top:{p.y - b.y - 7}px"
								data-input={k}
								role="presentation"
							>
								<span class="bg-base-content/70 h-[10px] w-[10px] rounded-sm"
								></span>
								<span class="opacity-80">{c.desc}</span>
							</div>
						{/each}
						{#each n.outputs as c, k (k)}
							{@const p = outputPoint(b, k)}
							<div
								class="absolute flex items-center gap-1 {c.hidden
									? 'opacity-40'
									: ''}"
								style="right:-5px;top:{p.y - b.y - 7}px"
							>
								<span class="opacity-80">{c.desc}</span>
								<span
									class="bg-base-content/70 hover:bg-primary h-[10px] w-[10px] cursor-crosshair rounded-sm"
									role="presentation"
									title="Drag to an input to link"
									onpointerdown={(e) => startLink(e, n, "output", k, p)}
								></span>
							</div>
						{/each}
						{#each n.variables as v, k (k)}
							{@const p = variablePoint(b, k, n.variables.length + n.events.length)}
							<div
								class="absolute flex -translate-x-1/2 flex-col items-center {v.hidden
									? 'opacity-40'
									: ''}"
								style="left:{p.x - b.x}px;bottom:-5px"
							>
								<span class="max-w-16 truncate text-[10px] opacity-80"
									>{v.desc}</span
								>
								<span
									class="hover:bg-primary h-[10px] w-[10px] cursor-crosshair rounded-full {v.writeable
										? 'bg-warning'
										: 'bg-error/80'}"
									role="presentation"
									title="{v.type ?? 'variable'}: drag to a variable to link"
									onpointerdown={(e) => startLink(e, n, "variable", k, p)}
								></span>
							</div>
						{/each}
					</div>
				{/if}
			{/each}
		</div>
		<div
			class="pointer-events-none absolute right-2 bottom-2 text-right font-mono text-[10px] opacity-50"
		>
			drag background pan · wheel zoom · drag title move · drag connector link<br />
			Alt+click or Delete a link to break it · double-click a sequence to enter · F fit<br />
			right-click or A to add a node · Delete removes the selected node
		</div>
		{#if adding}
			<div
				class="bg-base-100 border-base-300 absolute z-20 flex w-80 cursor-default flex-col rounded border shadow-lg"
				style="left:{adding.left}px;top:{adding.top}px;max-height:330px"
				data-palette
				role="dialog"
				tabindex="-1"
				aria-label="Add a node"
				onpointerdown={(e) => e.stopPropagation()}
				onwheel={(e) => e.stopPropagation()}
			>
				<input
					class="input input-xs m-1.5"
					placeholder="Add a node: type to filter (delay, toggle, touch…)"
					bind:value={paletteFilter}
					onkeydown={(e) => {
						if (e.key === "Enter" && shownPalette[0]) void pick(shownPalette[0]);
						if (e.key === "Escape") adding = null;
					}}
					{@attach (el) => el.focus()}
				/>
				{#if map.selected?.level === map.kismetLevel && map.actor()}
					<div class="px-2 pb-1 text-[10px] opacity-60">
						Events fire from, and object variables hold, {map.actor()?.name} (selected in
						the scene).
					</div>
				{/if}
				<div class="min-h-0 flex-1 overflow-auto pb-1 text-xs">
					{#if !map.palette}
						<div class="flex items-center gap-2 p-2 opacity-60">
							<span class="loading loading-xs loading-spinner"></span> Listing the nodes
							the game uses…
						</div>
					{/if}
					{#each shownPalette.slice(0, 200) as c (c.class)}
						<button
							class="hover:bg-base-300 flex w-full items-center gap-2 px-2 py-0.5 text-left"
							onclick={() => pick(c)}
							title="Copied from {c.example.path.split('/').at(-1)} (export {c.example
								.index})"
						>
							<span
								class="h-2 w-2 shrink-0 rounded-full {c.kind === 'event'
									? 'bg-error'
									: c.kind === 'condition'
										? 'bg-warning'
										: c.kind === 'variable'
											? 'bg-error/50'
											: 'bg-info'}"
							></span>
							<span class="flex-1 truncate font-mono">{c.class}</span>
							<span class="opacity-40">{c.count}</span>
						</button>
					{/each}
				</div>
			</div>
		{/if}
		{#if nodes.length === 0}
			<div class="absolute inset-0 flex items-center justify-center text-sm opacity-60">
				{map.graph ? "This sequence is empty." : "No sequence."}
			</div>
		{/if}
	</div>
</div>
