<!--
	A Matinee timeline under the 3D view: groups, their tracks and every key
	list (curves and timed keys). Play or scrub to preview the movements in
	the view (driven actors move, every movement draws its path); drag a key
	to retime it, click it to edit its time and value, double-click a row to
	add a key there, Delete removes the selected key.
-->
<script lang="ts">
	import { Pause, Play, SkipBack, Trash2 } from "@lucide/svelte";
	import { onDestroy } from "svelte";
	import { map } from "$lib/studio/map.svelte";
	import type { MatineeKey, MatineeTrack } from "../../../../electron/src/ipc/contract";

	const tl = $derived(map.matinee);
	const length = $derived(Math.max(tl?.length ?? 1, 0.01));
	let lane = $state<HTMLDivElement | null>(null);
	let width = $state(600);
	let loop = $state(true);
	let frame = 0;

	$effect(() => {
		if (!lane) return;
		const ro = new ResizeObserver(() => (width = lane?.clientWidth ?? 600));
		ro.observe(lane);
		return () => ro.disconnect();
	});

	const x = (t: number) => (t / length) * width;
	const timeAt = (clientX: number) => {
		const r = lane!.getBoundingClientRect();
		return Math.min(length, Math.max(0, ((clientX - r.left) / r.width) * length));
	};

	function play() {
		if (map.matineePlaying) {
			map.matineePlaying = false;
			return;
		}
		if (map.matineeTime >= length) map.matineeTime = 0;
		map.matineePlaying = true;
		let last = performance.now();
		const step = (now: number) => {
			if (!map.matineePlaying) return;
			const t = map.matineeTime + (now - last) / 1000;
			last = now;
			if (t >= length) {
				if (loop) map.matineeTime = 0;
				else {
					map.matineeTime = length;
					map.matineePlaying = false;
					return;
				}
			} else map.matineeTime = t;
			frame = requestAnimationFrame(step);
		};
		frame = requestAnimationFrame(step);
	}
	onDestroy(() => {
		cancelAnimationFrame(frame);
		map.matineePlaying = false;
	});

	function scrub(e: PointerEvent) {
		const el = e.currentTarget as HTMLElement;
		el.setPointerCapture(e.pointerId);
		map.matineeTime = timeAt(e.clientX);
		const move = (ev: PointerEvent) => (map.matineeTime = timeAt(ev.clientX));
		const up = () => {
			el.removeEventListener("pointermove", move);
			el.removeEventListener("pointerup", up);
		};
		el.addEventListener("pointermove", move);
		el.addEventListener("pointerup", up);
	}

	/** Drag a key in time (between its neighbours); a click selects it. */
	function dragKey(e: PointerEvent, track: number, curve: string, keys: MatineeKey[], k: number) {
		e.stopPropagation();
		map.matineeKey = { track, curve, key: k };
		const el = e.currentTarget as HTMLElement;
		el.setPointerCapture(e.pointerId);
		const lo = k > 0 ? keys[k - 1].time + 0.001 : 0;
		const hi = k < keys.length - 1 ? keys[k + 1].time - 0.001 : length;
		const start = e.clientX;
		let time = keys[k].time;
		let moved = false;
		const move = (ev: PointerEvent) => {
			if (!moved && Math.abs(ev.clientX - start) < 3) return;
			moved = true;
			time = Math.min(hi, Math.max(lo, timeAt(ev.clientX)));
			el.style.left = `${x(time)}px`;
			map.matineeTime = time;
		};
		const up = () => {
			el.removeEventListener("pointermove", move);
			el.removeEventListener("pointerup", up);
			if (moved) void map.setKey(track, curve, k, { time: Math.round(time * 1000) / 1000 });
		};
		el.addEventListener("pointermove", move);
		el.addEventListener("pointerup", up);
	}

	const selected = $derived.by(() => {
		const s = map.matineeKey;
		if (!s || !tl) return null;
		for (const g of tl.groups) {
			const track = g.tracks.find((t) => t.export === s.track);
			const curve = track?.curves.find((c) => c.path === s.curve);
			const key = curve?.keys[s.key];
			if (track && curve && key) return { track, curve, key, index: s.key };
		}
		return null;
	});

	let editTime = $state("");
	let editValue = $state<string[]>([]);
	$effect(() => {
		const s = selected;
		editTime = s ? String(Math.round(s.key.time * 1000) / 1000) : "";
		const v = s?.key.value;
		editValue = Array.isArray(v)
			? v.map((n) => String(Math.round(n * 1000) / 1000))
			: typeof v === "number"
				? [String(v)]
				: [];
	});

	async function applyEdit() {
		const s = selected;
		if (!s) return;
		const time = Number(editTime);
		const nums = editValue.map(Number);
		const change: { time?: number; value?: number | number[] } = {};
		if (Number.isFinite(time) && Math.abs(time - s.key.time) > 1e-6) change.time = time;
		if (s.curve.kind === "curve" && nums.length > 0 && nums.every(Number.isFinite)) {
			change.value = nums.length === 1 ? nums[0] : nums;
		}
		if (change.time === undefined && change.value === undefined) return;
		await map.setKey(s.track.export, s.curve.path, s.index, change);
	}

	function onKey(e: KeyboardEvent) {
		const t = e.target as HTMLElement | null;
		if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) {
			return;
		}
		if ((e.key === "Delete" || e.key === "Backspace") && selected) {
			e.preventDefault();
			void map.removeKey(selected.track.export, selected.curve.path, selected.index);
		} else if (e.key === " ") {
			e.preventDefault();
			play();
		}
	}

	const label = (tr: MatineeTrack) =>
		tr.moveAxis ? tr.moveAxis.replace(/^axis_/, "") : (tr.property ?? tr.title);
	const swatch = (c: number[] | null) => (c ? `rgb(${c[0]},${c[1]},${c[2]})` : "#a6adc8");
	const ticks = $derived.by(() => {
		const step = length > 20 ? 5 : length > 5 ? 1 : length > 1 ? 0.25 : 0.05;
		return Array.from({ length: Math.floor(length / step) + 1 }, (_, i) => i * step);
	});
</script>

<svelte:window onkeydown={onKey} />

{#if tl}
	<div class="border-base-300 bg-base-200 flex h-72 shrink-0 flex-col border-t text-xs">
		<div class="border-base-300 flex h-8 items-center gap-1 border-b px-2">
			<button
				class="btn btn-ghost btn-xs btn-square"
				onclick={() => (map.matineeTime = 0)}
				title="Start"><SkipBack size={12} /></button
			>
			<button
				class="btn btn-ghost btn-xs btn-square"
				onclick={play}
				title="Play / pause (Space)"
			>
				{#if map.matineePlaying}<Pause size={12} />{:else}<Play size={12} />{/if}
			</button>
			<label class="flex items-center gap-1 opacity-70">
				<input type="checkbox" class="checkbox checkbox-xs" bind:checked={loop} /> loop
			</label>
			<span class="ml-2 font-mono">{map.matineeTime.toFixed(2)} / {length.toFixed(2)} s</span>
			<span class="ml-2 truncate font-mono opacity-50">{tl.name}</span>
			<div class="flex-1"></div>
			{#if selected}
				<span class="opacity-60">{selected.curve.name} key {selected.index}</span>
				<input
					class="input input-xs w-20 font-mono"
					bind:value={editTime}
					onchange={applyEdit}
					title="Time (s)"
				/>
				{#each editValue as _, k (k)}
					<input
						class="input input-xs w-20 font-mono"
						bind:value={editValue[k]}
						onchange={applyEdit}
						title={selected.curve.kind === "curve" ? `value ${k}` : "label"}
					/>
				{/each}
				{#if selected.curve.kind === "keys"}
					<span class="font-mono opacity-60">{String(selected.key.value ?? "")}</span>
				{/if}
				<button
					class="btn btn-ghost btn-xs btn-square text-error"
					onclick={() =>
						map.removeKey(selected.track.export, selected.curve.path, selected.index)}
					title="Remove the key (Delete)"><Trash2 size={12} /></button
				>
			{:else}
				<span class="opacity-50"
					>click a key to edit it · double-click a row to add one</span
				>
			{/if}
		</div>
		<div class="flex min-h-0 flex-1 overflow-auto">
			<div class="border-base-300 w-56 shrink-0 border-r">
				<div class="h-6"></div>
				{#each tl.groups as g (g.export)}
					<div class="bg-base-300/50 flex h-6 items-center gap-1.5 px-2 font-semibold">
						<span class="h-2 w-2 rounded-sm" style="background:{swatch(g.color)}"
						></span>
						<span class="truncate">{g.name || g.class}</span>
						{#if g.actors.length > 0}<span class="truncate font-normal opacity-50"
								>{g.actors.map((a) => a.name).join(", ")}</span
							>{/if}
					</div>
					{#each g.tracks as tr (tr.export)}
						{#each tr.curves as c (c.path)}
							<div
								class="flex h-6 items-center gap-1 truncate px-2 {tr.parent !== null
									? 'pl-5'
									: ''} {tr.disabled ? 'opacity-40' : ''}"
								title="{tr.class} · {c.path}"
							>
								<span class="truncate">{label(tr)}</span>
								<span class="truncate opacity-50">{c.name}</span>
							</div>
						{/each}
					{/each}
				{/each}
			</div>
			<div class="relative min-w-0 flex-1" bind:this={lane}>
				<div
					class="bg-base-300/30 relative h-6 cursor-ew-resize"
					onpointerdown={scrub}
					role="slider"
					tabindex="-1"
					aria-label="Time"
					aria-valuenow={map.matineeTime}
				>
					{#each ticks as t (t)}
						<span
							class="absolute top-0 h-full border-l border-white/10 pl-0.5 font-mono text-[9px] opacity-60"
							style="left:{x(t)}px">{Math.round(t * 100) / 100}</span
						>
					{/each}
				</div>
				{#each tl.groups as g (g.export)}
					<div class="bg-base-300/50 h-6"></div>
					{#each g.tracks as tr (tr.export)}
						{#each tr.curves as c (c.path)}
							<div
								class="border-base-300/50 relative h-6 border-b"
								role="presentation"
								ondblclick={(e) =>
									map.addKey(
										tr.export,
										c.path,
										Math.round(timeAt(e.clientX) * 1000) / 1000,
									)}
							>
								{#each c.keys as k, i (i)}
									{@const active =
										map.matineeKey?.track === tr.export &&
										map.matineeKey.curve === c.path &&
										map.matineeKey.key === i}
									<span
										class="absolute top-1.5 h-3 w-3 -translate-x-1/2 rotate-45 cursor-pointer border {active
											? 'bg-warning border-warning'
											: c.kind === 'curve'
												? 'bg-primary/70 border-primary'
												: 'bg-secondary/70 border-secondary'}"
										style="left:{x(k.time)}px"
										role="button"
										tabindex="-1"
										title="{k.time.toFixed(3)} s{k.value !== null
											? `: ${Array.isArray(k.value) ? k.value.map((v) => v.toFixed(1)).join(', ') : k.value}`
											: ''}"
										onpointerdown={(e) =>
											dragKey(e, tr.export, c.path, c.keys, i)}
									></span>
								{/each}
							</div>
						{/each}
					{/each}
				{/each}
				<div
					class="bg-warning pointer-events-none absolute top-0 bottom-0 w-px"
					style="left:{x(map.matineeTime)}px"
				></div>
			</div>
		</div>
	</div>
{/if}

<style>
	span[role="button"]:hover {
		filter: brightness(1.3);
	}
</style>
