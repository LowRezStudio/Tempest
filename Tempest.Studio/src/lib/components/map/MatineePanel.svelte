<!--
	The Matinee editor under the 3D view: groups and their tracks (add,
	rename, recolour, remove; drive the actor selected in the view), every
	key list on a timeline (click a key to select it, drag to retime,
	double-click a row to add a key there, K adds one at the playhead,
	Delete removes), the selected key or track in the strip below (time,
	value, interp mode, tangents, event/cut/sound fields; move frame,
	rotation mode…), a curve editor for a curve (`MatineeCurveEditor`), and
	playback with loop, section and rate. Playing or scrubbing previews the
	movements, scales, light colours and toggles in the view.
-->
<script lang="ts">
	import {
		Activity,
		Clapperboard,
		Link2,
		Pause,
		Play,
		Plus,
		Settings2,
		SkipBack,
		Trash2,
		Unlink,
	} from "@lucide/svelte";
	import { onDestroy } from "svelte";
	import { map } from "$lib/studio/map.svelte";
	import { MODES, type Mode } from "$lib/studio/matinee-math";
	import { matineeEd, TRACK_CLASSES, type KeyRef } from "$lib/studio/matinee.svelte";
	import MatineeCurveEditor from "./MatineeCurveEditor.svelte";
	import type {
		MatineeCurve,
		MatineeGroup,
		MatineeTrack,
	} from "../../../../electron/src/ipc/contract";

	const tl = $derived(map.matinee);
	const length = $derived(Math.max(tl?.length ?? 1, 0.01));
	const level = $derived(map.matineeAt?.level ?? null);
	let lane = $state<HTMLDivElement | null>(null);
	let width = $state(600);
	let frame = 0;
	let showSettings = $state(false);
	let newGroup = $state("");
	let renaming = $state<number | null>(null);
	let renameText = $state("");
	let addTrackFor = $state<number | null>(null);
	let trackClass = $state(TRACK_CLASSES[0].class);
	let trackProperty = $state("");

	$effect(() => {
		if (!lane) return;
		const ro = new ResizeObserver(() => (width = lane?.clientWidth ?? 600));
		ro.observe(lane);
		return () => ro.disconnect();
	});

	$effect(() => {
		if (level && matineeEd.listFor !== level) void matineeEd.loadList(level);
	});

	const x = (t: number) => (t / length) * width;
	const timeAt = (clientX: number) => {
		const r = lane!.getBoundingClientRect();
		return Math.min(length, Math.max(0, ((clientX - r.left) / r.width) * length));
	};
	const round = (t: number) => Math.round(t * 1000) / 1000;

	/** The range playback covers: the whole timeline, or the editor section. */
	const range = $derived.by((): [number, number] => {
		if (!tl || !matineeEd.sectionOnly) return [0, length];
		const [a, b] = tl.section;
		return b > a ? [Math.max(0, a), Math.min(length, b)] : [0, length];
	});

	function play() {
		if (map.matineePlaying) {
			map.matineePlaying = false;
			return;
		}
		const [start, end] = range;
		if (map.matineeTime >= end || map.matineeTime < start) map.matineeTime = start;
		map.matineePlaying = true;
		let last = performance.now();
		const step = (now: number) => {
			if (!map.matineePlaying) return;
			const rate = matineeEd.rate ?? tl?.settings.playRate ?? 1;
			const t = map.matineeTime + ((now - last) / 1000) * rate;
			last = now;
			if (t >= end) {
				if (matineeEd.loop) map.matineeTime = start;
				else {
					map.matineeTime = end;
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
	function dragKey(e: PointerEvent, track: number, curve: MatineeCurve, k: number) {
		e.stopPropagation();
		const keys = curve.keys;
		matineeEd.selectKey({ track, curve: curve.path, key: k });
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
			// Alt: past the neighbours (the key is re-sorted).
			time = ev.altKey ? timeAt(ev.clientX) : Math.min(hi, Math.max(lo, timeAt(ev.clientX)));
			el.style.left = `${x(time)}px`;
			map.matineeTime = time;
		};
		const up = () => {
			el.removeEventListener("pointermove", move);
			el.removeEventListener("pointerup", up);
			if (moved) {
				void matineeEd.setKey({ track, curve: curve.path, key: k }, { time: round(time) });
			}
		};
		el.addEventListener("pointermove", move);
		el.addEventListener("pointerup", up);
	}

	const selected = $derived.by(() => {
		const s = map.matineeKey;
		if (!s || !tl) return null;
		const track = matineeEd.trackOf(s.track);
		const curve = track?.curves.find((c) => c.path === s.curve);
		const key = curve?.keys[s.key];
		return track && curve && key ? { track, curve, key, ref: s as KeyRef } : null;
	});
	const selTrack = $derived(matineeEd.trackOf(matineeEd.track));

	const fmt = (n: number) => String(Math.round(n * 1000) / 1000);
	const list = (v: unknown): number[] =>
		Array.isArray(v) ? (v as number[]) : typeof v === "number" ? [v] : [];

	let editTime = $state("");
	let editValue = $state<string[]>([]);
	let editArrive = $state<string[]>([]);
	let editLeave = $state<string[]>([]);
	let editFields = $state<Record<string, string>>({});
	$effect(() => {
		const s = selected;
		editTime = s ? fmt(s.key.time) : "";
		editValue = list(s?.key.value).map(fmt);
		editArrive = list(s?.key.arrive).map(fmt);
		editLeave = list(s?.key.leave).map(fmt);
		const f: Record<string, string> = {};
		for (const [k, v] of Object.entries(s?.key.fields ?? {})) {
			f[k] =
				v && typeof v === "object" && "path" in (v as object)
					? String((v as { path: string }).path)
					: v === null
						? "None"
						: String(v);
		}
		editFields = f;
	});

	const nums = (a: string[]) => {
		const n = a.map(Number);
		return n.length && n.every(Number.isFinite) ? (n.length === 1 ? n[0] : n) : undefined;
	};

	async function applyKey() {
		const s = selected;
		if (!s) return;
		const time = Number(editTime);
		const change: Parameters<typeof matineeEd.setKey>[1] = {};
		if (Number.isFinite(time) && Math.abs(time - s.key.time) > 1e-6) change.time = time;
		if (s.curve.kind === "curve") {
			const v = nums(editValue);
			if (v !== undefined && JSON.stringify(v) !== JSON.stringify(s.key.value)) {
				change.value = v;
			}
		}
		if (change.time === undefined && change.value === undefined) return;
		await matineeEd.setKey(s.ref, change);
	}

	async function applyTangents() {
		const s = selected;
		if (!s) return;
		await matineeEd.setKey(s.ref, { arrive: nums(editArrive), leave: nums(editLeave) });
	}

	/** A timed key's member as `Editor::set` text: names quoted, numbers and booleans as they are. */
	function fieldText(name: string, text: string, old: unknown): string {
		if (typeof old === "number" || typeof old === "boolean") return text;
		if (old && typeof old === "object") return text; // an object path
		if (/name|group/i.test(name) && !/^'.*'$/.test(text)) return `'${text}'`;
		return text;
	}

	async function applyField(name: string) {
		const s = selected;
		if (!s) return;
		const old = s.key.fields?.[name];
		await matineeEd.setKey(s.ref, {
			fields: { [name]: fieldText(name, editFields[name], old) },
		});
	}

	function onKey(e: KeyboardEvent) {
		const t = e.target as HTMLElement | null;
		if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) {
			return;
		}
		if (t?.closest("[data-map-kismet]")) return;
		if ((e.key === "Delete" || e.key === "Backspace") && selected) {
			e.preventDefault();
			void matineeEd.removeKey(selected.ref);
		} else if (e.key === " ") {
			e.preventDefault();
			play();
		} else if (e.key === "Home") {
			map.matineeTime = range[0];
		} else if (e.key === "End") {
			map.matineeTime = range[1];
		}
	}

	const label = (tr: MatineeTrack) =>
		tr.moveAxis ? tr.moveAxis.replace(/^axis_/, "") : (tr.property ?? tr.title);
	const swatch = (c: number[] | null) => (c ? `rgb(${c[0]},${c[1]},${c[2]})` : "#a6adc8");
	const hex = (c: number[] | null) =>
		`#${(c ?? [166, 173, 200])
			.slice(0, 3)
			.map((v) => Math.round(v).toString(16).padStart(2, "0"))
			.join("")}`;
	const ticks = $derived.by(() => {
		const step = length > 20 ? 5 : length > 5 ? 1 : length > 1 ? 0.25 : 0.05;
		return Array.from({ length: Math.floor(length / step) + 1 }, (_, i) => i * step);
	});

	/** The actor selected in the view, when it is in the timeline's level. */
	const viewActor = $derived(
		map.selected && map.selected.level === level ? map.actor(map.selected) : null,
	);
	const viewActorExport = $derived(
		viewActor
			? viewActor.edit === "component"
				? (viewActor.parent ?? null)
				: viewActor.export
			: null,
	);

	function startRename(g: MatineeGroup) {
		renaming = g.export;
		renameText = g.name;
	}
	async function commitRename(g: MatineeGroup) {
		const name = renameText.trim();
		renaming = null;
		if (name && name !== g.name) await matineeEd.setGroup(g.export, { name });
	}
	async function addGroup() {
		const name = newGroup.trim() || `Group${(tl?.groups.length ?? 0) + 1}`;
		newGroup = "";
		await matineeEd.addGroup(name);
	}
	async function addTrack(g: MatineeGroup) {
		addTrackFor = null;
		const spec = TRACK_CLASSES.find((c) => c.class === trackClass);
		await matineeEd.addTrack(
			g.export,
			trackClass,
			spec?.property ? trackProperty.trim() || spec.property[0] : undefined,
		);
	}
	const keyLabel = (c: MatineeCurve, v: unknown) =>
		c.kind === "keys" && typeof v === "string" ? v.replace(/^etta_|^evta_/, "") : "";
	const spec = $derived(TRACK_CLASSES.find((c) => c.class === trackClass));
	const isStaticActor = (cls: string | undefined) =>
		/^staticmeshactor$|collection/i.test(cls ?? "");
</script>

<svelte:window onkeydown={onKey} />

{#if tl}
	<div class="border-base-300 bg-base-200 flex h-96 shrink-0 flex-col border-t text-xs">
		<!-- Toolbar -->
		<div
			class="border-base-300 flex min-h-8 shrink-0 flex-wrap items-center gap-1 border-b px-2"
		>
			<Clapperboard size={12} class="opacity-60" />
			<select
				class="select select-xs w-44 shrink-0 overflow-hidden text-ellipsis"
				value={map.matineeAt?.index ?? ""}
				onchange={(e) => {
					const v = Number((e.currentTarget as HTMLSelectElement).value);
					if (level && Number.isFinite(v)) void map.openMatinee(level, v);
				}}
				title="The level's Matinees"
			>
				{#if !matineeEd.list?.some((m) => m.export === map.matineeAt?.index)}
					<option value={map.matineeAt?.index}>{tl.name}</option>
				{/if}
				{#each matineeEd.list ?? [] as m (m.export)}
					<option value={m.export}
						>{m.comment ?? m.path.split(".").at(-1)}{m.groups?.length
							? ` · ${m.groups.join(", ")}`
							: ""}</option
					>
				{/each}
			</select>
			<button
				class="btn btn-ghost btn-xs"
				disabled={!level}
				onclick={() =>
					level && matineeEd.create(level, { actor: viewActorExport ?? undefined })}
				title={viewActorExport !== null
					? `A new Matinee moving ${viewActor?.name}`
					: "A new, empty Matinee in this level's sequence"}
				><Plus size={12} /> New</button
			>
			<div class="divider divider-horizontal mx-0"></div>
			<button
				class="btn btn-ghost btn-xs btn-square"
				onclick={() => (map.matineeTime = range[0])}
				title="Start (Home)"><SkipBack size={12} /></button
			>
			<button
				class="btn btn-ghost btn-xs btn-square"
				onclick={play}
				title="Play / pause (Space)"
			>
				{#if map.matineePlaying}<Pause size={12} />{:else}<Play size={12} />{/if}
			</button>
			<label class="flex items-center gap-1 opacity-80" title="Loop the preview">
				<input type="checkbox" class="checkbox checkbox-xs" bind:checked={matineeEd.loop} /> loop
			</label>
			<label
				class="flex items-center gap-1 opacity-80"
				title="Play only the section (EdSectionStart..End)"
			>
				<input
					type="checkbox"
					class="checkbox checkbox-xs"
					bind:checked={matineeEd.sectionOnly}
				/> section
			</label>
			<label
				class="flex items-center gap-1 opacity-80"
				title="Preview speed (the action's PlayRate when empty)"
			>
				×<input
					class="input input-xs w-12 font-mono"
					placeholder={fmt(tl.settings.playRate)}
					value={matineeEd.rate ?? ""}
					onchange={(e) => {
						const v = Number((e.currentTarget as HTMLInputElement).value);
						matineeEd.rate =
							(e.currentTarget as HTMLInputElement).value && v > 0 ? v : null;
					}}
				/>
			</label>
			<span class="ml-2 font-mono">{map.matineeTime.toFixed(2)} /</span>
			<input
				class="input input-xs w-16 font-mono"
				value={fmt(tl.length)}
				title="InterpLength (s)"
				onchange={(e) =>
					matineeEd.setLength(Number((e.currentTarget as HTMLInputElement).value))}
			/>
			<span class="opacity-60">s</span>
			<div class="flex-1"></div>
			<button
				class="btn btn-ghost btn-xs {showSettings ? 'btn-active' : ''}"
				onclick={() => (showSettings = !showSettings)}
				disabled={tl.action === null}
				title="The Matinee action's settings"><Settings2 size={12} /> Action</button
			>
			<input
				class="input input-xs w-28"
				placeholder="new group name"
				bind:value={newGroup}
				onkeydown={(e) => e.key === "Enter" && addGroup()}
			/>
			<button class="btn btn-ghost btn-xs" onclick={addGroup} title="Add a group"
				><Plus size={12} /> Group</button
			>
			{#if !tl.groups.some((g) => g.director)}
				<button
					class="btn btn-ghost btn-xs"
					onclick={() => matineeEd.addGroup("DirGroup", true)}
					title="Add the director group (camera cuts, fades)">Director</button
				>
			{/if}
		</div>
		{#if showSettings && tl.action !== null}
			{@const a = tl.action}
			{@const st = tl.settings}
			<div class="border-base-300 flex flex-wrap items-center gap-3 border-b px-2 py-1">
				{#each [["bLooping", "looping", "Loop"], ["bRewindOnPlay", "rewindOnPlay", "Rewind on play"], ["bRewindIfAlreadyPlaying", "rewindIfAlreadyPlaying", "Rewind if playing"], ["bNoResetOnRewind", "noResetOnRewind", "No reset on rewind"], ["bReversePlayback", "reversePlayback", "Reverse"], ["bClientSideOnly", "clientSideOnly", "Client side only"], ["bForceStartPos", "forceStartPos", "Force start position"]] as const as [prop, key, text] (prop)}
					<label class="flex items-center gap-1">
						<input
							type="checkbox"
							class="checkbox checkbox-xs"
							checked={st[key]}
							onchange={(e) =>
								matineeEd.set(a, {
									[prop]: (e.currentTarget as HTMLInputElement).checked,
								})}
						/>{text}
					</label>
				{/each}
				<label class="flex items-center gap-1"
					>start at <input
						class="input input-xs w-14 font-mono"
						value={fmt(st.forceStartPosition)}
						onchange={(e) =>
							matineeEd.set(a, {
								ForceStartPosition: Number(
									(e.currentTarget as HTMLInputElement).value,
								),
							})}
					/></label
				>
				<label class="flex items-center gap-1"
					>PlayRate <input
						class="input input-xs w-14 font-mono"
						value={fmt(st.playRate)}
						onchange={(e) =>
							matineeEd.set(a, {
								PlayRate: Number((e.currentTarget as HTMLInputElement).value),
							})}
					/></label
				>
				<label class="flex items-center gap-1"
					>section <input
						class="input input-xs w-14 font-mono"
						value={fmt(tl.section[0])}
						onchange={(e) =>
							matineeEd.set(tl.data, {
								EdSectionStart: Number((e.currentTarget as HTMLInputElement).value),
							})}
					/>–<input
						class="input input-xs w-14 font-mono"
						value={fmt(tl.section[1])}
						onchange={(e) =>
							matineeEd.set(tl.data, {
								EdSectionEnd: Number((e.currentTarget as HTMLInputElement).value),
							})}
					/></label
				>
			</div>
		{/if}
		<!-- Groups, tracks and lanes -->
		<div class="flex min-h-0 flex-1 overflow-auto">
			<div class="border-base-300 w-72 shrink-0 border-r">
				<div class="h-6"></div>
				{#each tl.groups as g (g.export)}
					<div
						class="bg-base-300/50 group flex h-6 items-center gap-1.5 px-2 font-semibold {matineeEd.group ===
						g.export
							? 'ring-primary/50 ring-1 ring-inset'
							: ''}"
						role="presentation"
						onclick={() => (matineeEd.group = g.export)}
					>
						<input
							type="color"
							class="h-3 w-3 shrink-0 cursor-pointer rounded-sm border-0 p-0"
							value={hex(g.color)}
							style="background:{swatch(g.color)}"
							title="Group colour"
							disabled={g.director}
							onchange={(e) => {
								const h = (e.currentTarget as HTMLInputElement).value;
								void matineeEd.setGroup(g.export, {
									color: [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16)),
								});
							}}
						/>
						{#if renaming === g.export}
							<!-- svelte-ignore a11y_autofocus -->
							<input
								class="input input-xs h-5 min-w-0 flex-1"
								bind:value={renameText}
								autofocus
								onblur={() => commitRename(g)}
								onkeydown={(e) => {
									if (e.key === "Enter") void commitRename(g);
									else if (e.key === "Escape") renaming = null;
								}}
							/>
						{:else}
							<span
								class="truncate"
								role="presentation"
								ondblclick={() => !g.director && startRename(g)}
								title={g.director ? "Director group" : "Double-click to rename"}
								>{g.name || g.class}</span
							>
						{/if}
						{#if g.actors.length > 0}<span class="truncate font-normal opacity-50"
								>{g.actors.map((a) => a.name).join(", ")}</span
							>{/if}
						<div
							class="ml-auto flex shrink-0 items-center opacity-0 group-hover:opacity-100"
						>
							{#if !g.director && viewActorExport !== null && tl.action !== null}
								{@const linked = g.actors.some((a) => a.export === viewActorExport)}
								<button
									class="btn btn-ghost btn-xs btn-square h-5 min-h-0"
									onclick={(e) => {
										e.stopPropagation();
										void matineeEd.connect(g.export, viewActorExport!, linked);
									}}
									title={linked
										? `Stop driving ${viewActor?.name}`
										: `Drive ${viewActor?.name} (the actor selected in the view)${isStaticActor(viewActor?.class) ? " — static meshes do not move in game: use an InterpActor" : ""}`}
								>
									{#if linked}<Unlink size={11} />{:else}<Link2 size={11} />{/if}
								</button>
							{/if}
							<button
								class="btn btn-ghost btn-xs btn-square h-5 min-h-0"
								onclick={(e) => {
									e.stopPropagation();
									addTrackFor = addTrackFor === g.export ? null : g.export;
									trackClass = g.director
										? "InterpTrackDirector"
										: TRACK_CLASSES[0].class;
									trackProperty = "";
								}}
								title="Add a track"><Plus size={11} /></button
							>
							<button
								class="btn btn-ghost btn-xs btn-square text-error h-5 min-h-0"
								onclick={(e) => {
									e.stopPropagation();
									void matineeEd.removeGroup(g.export);
								}}
								title="Remove the group and its tracks"><Trash2 size={11} /></button
							>
						</div>
					</div>
					{#if addTrackFor === g.export}
						<div class="bg-base-100 flex h-6 items-center gap-1 px-2">
							<select class="select select-xs min-w-0 flex-1" bind:value={trackClass}>
								{#each TRACK_CLASSES.filter((c) => !c.director || g.director) as c (c.class)}
									<option value={c.class}>{c.label}</option>
								{/each}
							</select>
							{#if spec?.property}
								<input
									class="input input-xs w-24"
									list="matinee-props"
									placeholder={spec.property[0] ?? "parameter"}
									bind:value={trackProperty}
								/>
								<datalist id="matinee-props">
									{#each spec.property as p (p)}<option value={p}></option>{/each}
								</datalist>
							{/if}
							<button
								class="btn btn-primary btn-xs h-5 min-h-0"
								onclick={() => addTrack(g)}>Add</button
							>
						</div>
					{/if}
					{#each g.tracks as tr (tr.export)}
						{#each tr.curves as c, ci (c.path)}
							<div
								class="group flex h-6 items-center gap-1 truncate px-2 {tr.parent !==
								null
									? 'pl-5'
									: ''} {tr.disabled ? 'opacity-40' : ''} {matineeEd.track ===
								tr.export
									? 'bg-primary/10'
									: ''}"
								role="presentation"
								onclick={() => {
									matineeEd.track = tr.export;
									matineeEd.group = g.export;
									map.matineeKey = null;
								}}
								title="{tr.class} · {c.path}"
							>
								{#if ci === 0}
									<input
										type="checkbox"
										class="checkbox checkbox-xs"
										checked={!tr.disabled}
										title="Enabled (bDisableTrack)"
										onclick={(e) => e.stopPropagation()}
										onchange={(e) =>
											matineeEd.set(tr.export, {
												bDisableTrack: !(
													e.currentTarget as HTMLInputElement
												).checked,
											})}
									/>
								{:else}<span class="w-3"></span>{/if}
								<span class="truncate">{label(tr)}</span>
								<span class="truncate opacity-50">{c.name}</span>
								<div
									class="ml-auto flex shrink-0 items-center opacity-0 group-hover:opacity-100"
								>
									{#if c.kind === "curve"}
										<button
											class="btn btn-ghost btn-xs btn-square h-5 min-h-0 {matineeEd
												.curve?.path === c.path &&
											matineeEd.curve.track === tr.export
												? 'btn-active'
												: ''}"
											onclick={(e) => {
												e.stopPropagation();
												matineeEd.curve = {
													track: tr.export,
													path: c.path,
												};
											}}
											title="Curve editor"><Activity size={11} /></button
										>
									{/if}
									<button
										class="btn btn-ghost btn-xs btn-square h-5 min-h-0"
										onclick={(e) => {
											e.stopPropagation();
											matineeEd.track = tr.export;
											void (ci === 0
												? matineeEd.keyAtPlayhead(tr.export)
												: matineeEd.addKey(
														tr.export,
														c.path,
														round(map.matineeTime),
													));
										}}
										title="Add a key at the playhead (K){/move/i.test(tr.class)
											? ' from where the actor is'
											: ''}">K</button
									>
									{#if ci === 0}
										<button
											class="btn btn-ghost btn-xs btn-square text-error h-5 min-h-0"
											onclick={(e) => {
												e.stopPropagation();
												void matineeEd.removeTrack(tr.export);
											}}
											title="Remove the track"><Trash2 size={11} /></button
										>
									{/if}
								</div>
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
					{#if tl.section[1] > tl.section[0]}
						<div
							class="bg-info/15 pointer-events-none absolute top-0 h-full"
							style="left:{x(tl.section[0])}px;width:{x(tl.section[1]) -
								x(tl.section[0])}px"
						></div>
					{/if}
					{#each ticks as t (t)}
						<span
							class="absolute top-0 h-full border-l border-white/10 pl-0.5 font-mono text-[9px] opacity-60"
							style="left:{x(t)}px">{Math.round(t * 100) / 100}</span
						>
					{/each}
				</div>
				{#each tl.groups as g (g.export)}
					<div class="bg-base-300/50 h-6"></div>
					{#if addTrackFor === g.export}<div class="h-6"></div>{/if}
					{#each g.tracks as tr (tr.export)}
						{#each tr.curves as c (c.path)}
							<div
								class="border-base-300/50 relative h-6 border-b"
								role="presentation"
								ondblclick={(e) =>
									matineeEd.addKey(tr.export, c.path, round(timeAt(e.clientX)))}
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
											: ''}{k.mode ? ` (${k.mode.replace('CIM_', '')})` : ''}"
										onpointerdown={(e) => dragKey(e, tr.export, c, i)}
									></span>
									{#if keyLabel(c, k.value)}
										<span
											class="pointer-events-none absolute top-1 truncate pl-2 font-mono text-[9px] opacity-70"
											style="left:{x(k.time)}px;max-width:120px"
											>{keyLabel(c, k.value)}</span
										>
									{/if}
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
		{#if matineeEd.curve}<div class="flex h-44 shrink-0"><MatineeCurveEditor /></div>{/if}
		<!-- The selected key, or track -->
		<div
			class="border-base-300 flex min-h-8 shrink-0 flex-wrap items-center gap-1 border-t px-2 py-1"
		>
			{#if selected}
				{@const s = selected}
				<span class="opacity-60">{label(s.track)} · {s.curve.name} key {s.ref.key}</span>
				<label class="flex items-center gap-1"
					>t <input
						class="input input-xs w-16 font-mono"
						bind:value={editTime}
						onchange={applyKey}
					/></label
				>
				{#if s.curve.kind === "curve"}
					{#each editValue as _, k (k)}
						<input
							class="input input-xs w-20 font-mono"
							bind:value={editValue[k]}
							onchange={applyKey}
							title={s.curve.components?.[k] ?? "value"}
						/>
					{/each}
					<select
						class="select select-xs w-28"
						value={s.key.mode ?? "CIM_Linear"}
						onchange={(e) =>
							matineeEd.setKey(s.ref, {
								mode: (e.currentTarget as HTMLSelectElement).value as Mode,
							})}
						title="Interp mode (a move key's mode applies to its position and rotation)"
					>
						{#each MODES as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
					</select>
					{#if s.key.mode !== "CIM_Linear" && s.key.mode !== "CIM_Constant"}
						<span class="opacity-60">in</span>
						{#each editArrive as _, k (k)}
							<input
								class="input input-xs w-16 font-mono"
								bind:value={editArrive[k]}
								onchange={applyTangents}
								title="Arrive tangent"
							/>
						{/each}
						<span class="opacity-60">out</span>
						{#each editLeave as _, k (k)}
							<input
								class="input input-xs w-16 font-mono"
								bind:value={editLeave[k]}
								onchange={applyTangents}
								title="Leave tangent"
							/>
						{/each}
						<button
							class="btn btn-ghost btn-xs"
							onclick={() => matineeEd.autoTangents(s.ref)}
							title="Auto clamped tangents">Auto</button
						>
					{/if}
				{:else}
					{#each Object.keys(editFields) as f (f)}
						<label class="flex items-center gap-1"
							><span class="opacity-60">{f}</span>
							<input
								class="input input-xs w-32 font-mono"
								bind:value={editFields[f]}
								onchange={() => applyField(f)}
							/></label
						>
					{/each}
				{/if}
				<button
					class="btn btn-ghost btn-xs btn-square text-error ml-auto"
					onclick={() => matineeEd.removeKey(s.ref)}
					title="Remove the key (Delete)"><Trash2 size={12} /></button
				>
			{:else if selTrack}
				{@const t = selTrack}
				<span class="opacity-60">{t.class}</span>
				<label class="flex items-center gap-1"
					>title <input
						class="input input-xs w-28"
						value={t.title}
						onchange={(e) =>
							matineeEd.set(t.export, {
								TrackTitle: `"${(e.currentTarget as HTMLInputElement).value}"`,
							})}
					/></label
				>
				{#if t.moveFrame !== null}
					<select
						class="select select-xs w-36"
						value={t.moveFrame}
						onchange={(e) =>
							matineeEd.set(t.export, {
								MoveFrame: (e.currentTarget as HTMLSelectElement).value,
							})}
						title="MoveFrame: keys in world space, or relative to where the actor starts"
					>
						<option value="imf_world">World</option>
						<option value="imf_relativetoinitial">Relative to initial</option>
					</select>
					<select
						class="select select-xs w-32"
						value={t.rotMode ?? "imr_keyframed"}
						onchange={(e) =>
							matineeEd.set(t.export, {
								RotMode: (e.currentTarget as HTMLSelectElement).value,
							})}
						title="RotMode"
					>
						<option value="imr_keyframed">Keyframed</option>
						<option value="imr_lookatgroup">Look at group</option>
						<option value="imr_ignore">Ignore</option>
					</select>
					{#if t.rotMode === "imr_lookatgroup"}
						<select
							class="select select-xs w-28"
							value={t.lookAtGroup ?? ""}
							onchange={(e) =>
								matineeEd.set(t.export, {
									LookAtGroupName: `'${(e.currentTarget as HTMLSelectElement).value}'`,
								})}
						>
							<option value="">(none)</option>
							{#each tl.groups.filter((x) => !x.director) as x (x.export)}<option
									value={x.name}>{x.name}</option
								>{/each}
						</select>
					{/if}
					<label
						class="flex items-center gap-1"
						title="bUseRawActorTMforRelativeToInitial"
					>
						<input
							type="checkbox"
							class="checkbox checkbox-xs"
							checked={t.rawInitial}
							onchange={(e) =>
								matineeEd.set(t.export, {
									bUseRawActorTMforRelativeToInitial: (
										e.currentTarget as HTMLInputElement
									).checked,
								})}
						/>raw initial
					</label>
				{/if}
				{#if t.property !== null}
					<label class="flex items-center gap-1"
						>property <input
							class="input input-xs w-28"
							value={t.property}
							onchange={(e) =>
								matineeEd.set(t.export, {
									[/materialparam|particleparam/i.test(t.class)
										? "ParamName"
										: "PropertyName"]:
										`'${(e.currentTarget as HTMLInputElement).value}'`,
								})}
						/></label
					>
				{/if}
				<span class="ml-auto opacity-50"
					>K: key at the playhead{t.moveFrame !== null
						? " · moving the actor with the gizmo keys it"
						: ""}</span
				>
			{:else}
				<span class="opacity-50"
					>click a key to edit it · double-click a row to add one · K adds one at the
					playhead on the selected track · in this view, moving a driven actor keys its
					movement</span
				>
			{/if}
		</div>
	</div>
{/if}

<style>
	span[role="button"]:hover {
		filter: brightness(1.3);
	}
</style>
