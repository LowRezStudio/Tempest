<!--
	An AnimSequence played on a skeletal mesh (the AnimSet's preview mesh by
	default): play, scrub, speed, loop, skeleton; the notifies on a timeline
	(drag to move, right-click to copy, open or remove); trim, retime and the
	rate scale. The backend decodes the compressed keys (`anim.preview` sends
	a GLB with the mesh and the animation) and writes the edits (`anim.edit`).
-->
<script lang="ts">
	import { Focus, Pause, Play, Repeat, Scissors, Timer } from "@lucide/svelte";
	import { onDestroy, untrack } from "svelte";
	import * as THREE from "three";
	import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
	import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
	import { bytes } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { AnimInfo, AnimNotify } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();
	/** The sequence shown: `index`, or one of the set's when `index` is an AnimSet. */
	let seq = $state(0);

	let canvas = $state<HTMLCanvasElement | null>(null);
	let host = $state<HTMLDivElement | null>(null);
	let timeline = $state<HTMLDivElement | null>(null);
	let info = $state<AnimInfo | null>(null);
	let meshKey = $state("");
	let error = $state<string | null>(null);
	let loading = $state(false);
	let busy = $state(false);
	let playing = $state(true);
	let loop = $state(true);
	let speed = $state(1);
	let showSkeleton = $state(false);
	let time = $state(0);
	let missing = $state<string[]>([]);
	let meshName = $state("");
	let trimStart = $state(0);
	let trimEnd = $state(0);
	let newLength = $state(0);
	let rateScale = $state(1);
	/** A notify being dragged: its index and the time it would move to. */
	let dragging = $state<{ index: number; time: number } | null>(null);

	let renderer: THREE.WebGLRenderer | null = null;
	let scene: THREE.Scene | null = null;
	let camera: THREE.PerspectiveCamera | null = null;
	let controls: OrbitControls | null = null;
	let model: THREE.Object3D | null = null;
	let mixer: THREE.AnimationMixer | null = null;
	let action: THREE.AnimationAction | null = null;
	let skeletonHelper: THREE.SkeletonHelper | null = null;
	let grid: THREE.GridHelper | null = null;
	let frameHandle = 0;
	let resizeObserver: ResizeObserver | null = null;
	const clock = new THREE.Clock();
	/** `studio.edits` after the last load, so a change made elsewhere reloads. */
	let seenEdits = -1;

	const length = $derived(info?.length ?? 0);
	const fps = $derived(info?.fps ?? 30);
	const frames = $derived(info?.numFrames ?? 0);
	const frame = $derived(Math.min(Math.round(time * fps), Math.max(frames - 1, 0)));

	function setup() {
		if (!canvas || !host || renderer) return;
		renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
		renderer.setPixelRatio(window.devicePixelRatio);
		renderer.outputColorSpace = THREE.SRGBColorSpace;
		scene = new THREE.Scene();
		scene.background = new THREE.Color(0x11111b);
		camera = new THREE.PerspectiveCamera(45, 1, 0.01, 1000);
		camera.position.set(2, 1.5, 2);
		controls = new OrbitControls(camera, canvas);
		controls.enableDamping = true;
		scene.add(new THREE.HemisphereLight(0xffffff, 0x444466, 1.4));
		const key = new THREE.DirectionalLight(0xffffff, 2);
		key.position.set(3, 5, 4);
		scene.add(key);
		grid = new THREE.GridHelper(4, 16, 0x6c7086, 0x45475a);
		scene.add(grid);
		resizeObserver = new ResizeObserver(resize);
		resizeObserver.observe(host);
		resize();
		const tick = () => {
			frameHandle = requestAnimationFrame(tick);
			const dt = clock.getDelta();
			if (mixer && action) {
				if (playing && !dragging) {
					mixer.update(dt * speed * (info?.rateScale ?? 1));
					if (!loop && action.time >= length) playing = false;
				}
				time = action.time;
			}
			controls?.update();
			if (renderer && scene && camera) renderer.render(scene, camera);
		};
		tick();
	}

	function resize() {
		if (!host || !renderer || !camera) return;
		const w = host.clientWidth;
		const h = host.clientHeight;
		if (w === 0 || h === 0) return;
		renderer.setSize(w, h, false);
		camera.aspect = w / h;
		camera.updateProjectionMatrix();
	}

	function fit() {
		if (!model || !camera || !controls || !grid) return;
		const box = new THREE.Box3().setFromObject(model);
		if (box.isEmpty()) return;
		const size = box.getSize(new THREE.Vector3());
		const center = box.getCenter(new THREE.Vector3());
		const radius = Math.max(size.x, size.y, size.z, 0.01);
		controls.target.copy(center);
		camera.near = radius / 100;
		camera.far = radius * 100;
		camera.position
			.copy(center)
			.add(new THREE.Vector3(radius * 1.2, radius * 0.6, radius * 1.2));
		camera.updateProjectionMatrix();
		controls.update();
		grid.position.set(center.x, box.min.y, center.z);
		grid.scale.setScalar((Math.max(size.x, size.z) * 2) / 4);
	}

	/** Load the sequence's details and its preview on the chosen mesh. */
	async function load(keepTime = false) {
		loading = true;
		error = null;
		const at = keepTime ? time : 0;
		try {
			const i = await rpc("anim.info", { path, index: seq });
			info = i;
			seq = i.index;
			rateScale = i.rateScale;
			trimStart = 0;
			trimEnd = Math.max(i.numFrames - 1, 0);
			newLength = Number(i.length.toFixed(3));
			const chosen =
				i.meshes.find((m) => `${m.path}#${m.index}` === untrack(() => meshKey)) ??
				i.meshes[0];
			if (!chosen) {
				error = "No skeletal mesh in the workspace has this animation's bones.";
				return;
			}
			meshKey = `${chosen.path}#${chosen.index}`;
			const r = await rpc("anim.preview", {
				path,
				index: seq,
				mesh: { path: chosen.path, index: chosen.index },
				textureSize: 512,
			});
			missing = r.tracksWithoutBone;
			meshName = r.mesh.name;
			const glb = r.glb ?? new Uint8Array();
			const gltf = await new GLTFLoader().parseAsync(
				glb.buffer.slice(glb.byteOffset, glb.byteOffset + glb.byteLength) as ArrayBuffer,
				"",
			);
			setup();
			const fresh = !model;
			if (model && scene) {
				scene.remove(model);
				if (skeletonHelper) scene.remove(skeletonHelper);
			}
			model = gltf.scene;
			// Like the mesh viewer: both faces (the game's winding is flipped), matte.
			model.traverse((o) => {
				if (o instanceof THREE.Mesh) {
					const mats = Array.isArray(o.material) ? o.material : [o.material];
					for (const m of mats) {
						if (m instanceof THREE.MeshStandardMaterial) {
							m.side = THREE.DoubleSide;
							if (!m.map) m.color.set(0x9399b2);
							m.metalness = 0;
							m.roughness = 0.8;
						}
					}
				}
			});
			scene?.add(model);
			skeletonHelper = new THREE.SkeletonHelper(model);
			skeletonHelper.visible = showSkeleton;
			scene?.add(skeletonHelper);
			mixer = new THREE.AnimationMixer(model);
			const clip = gltf.animations[0];
			action = clip ? mixer.clipAction(clip) : null;
			if (action) {
				action.setLoop(THREE.LoopRepeat, Infinity);
				action.clampWhenFinished = true;
				action.play();
				action.time = Math.min(at, i.length);
				mixer.update(0);
			}
			if (fresh) fit();
			seenEdits = studio.edits;
		} catch (error_) {
			error = error_ instanceof Error ? error_.message : String(error_);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void path;
		const i = index;
		untrack(() => {
			seq = i;
			void load();
		});
	});

	// A change made elsewhere (Replace, undo, the property editor) reloads.
	$effect(() => {
		const e = studio.edits;
		if (seenEdits >= 0 && e !== seenEdits && !untrack(() => loading || busy)) {
			untrack(() => void load(true));
		}
	});

	// Read the toggles first: the helper and action do not exist on the first run.
	$effect(() => {
		const visible = showSkeleton;
		if (skeletonHelper) skeletonHelper.visible = visible;
	});

	$effect(() => {
		const mode = loop ? THREE.LoopRepeat : THREE.LoopOnce;
		action?.setLoop(mode, Infinity);
	});

	onDestroy(() => {
		cancelAnimationFrame(frameHandle);
		resizeObserver?.disconnect();
		renderer?.dispose();
	});

	function seek(t: number) {
		if (!action || !mixer) return;
		action.time = Math.max(0, Math.min(t, length));
		mixer.update(0);
		time = action.time;
	}

	function togglePlay() {
		if (!action) return;
		if (!playing && action.time >= length - 1e-3) seek(0);
		playing = !playing;
	}

	/** Run an edit of the sequence, then reload everything. */
	async function edit(label: string, run: () => Promise<unknown>) {
		busy = true;
		try {
			await run();
			studio.markDirty(path);
			await load(true);
		} catch (error_) {
			toasts.error(error_, label);
		} finally {
			busy = false;
		}
	}

	const timeAt = (clientX: number) => {
		if (!timeline || length <= 0) return 0;
		const r = timeline.getBoundingClientRect();
		return Math.max(0, Math.min(1, (clientX - r.left) / r.width)) * length;
	};

	function startDrag(event: PointerEvent, n: AnimNotify) {
		if (event.button !== 0) return;
		event.preventDefault();
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
		dragging = { index: n.index, time: n.time };
	}

	function moveDrag(event: PointerEvent) {
		if (dragging) dragging = { ...dragging, time: timeAt(event.clientX) };
	}

	async function endDrag() {
		const d = dragging;
		dragging = null;
		if (!d) return;
		const n = info?.notifies.find((x) => x.index === d.index);
		if (!n || Math.abs(n.time - d.time) < 1e-4) return;
		await edit("Move notify", () =>
			rpc("anim.edit", {
				path,
				index: seq,
				op: "notifyMove",
				notify: d.index,
				time: Number(d.time.toFixed(4)),
			}),
		);
	}

	function notifyName(n: AnimNotify): string {
		return n.notify.path.split(".").at(-1) ?? n.notify.path;
	}

	function notifyMenu(event: MouseEvent, n: AnimNotify) {
		ui.openContextMenu(event, [
			{ heading: `${notifyName(n)} at ${n.time.toFixed(3)} s` },
			{
				label: "Copy at the playhead",
				detail: `A new ${n.notify.class} at ${time.toFixed(3)} s`,
				run: () =>
					edit("Add notify", () =>
						rpc("anim.edit", {
							path,
							index: seq,
							op: "notifyAdd",
							notify: n.index,
							time: Number(time.toFixed(4)),
						}),
					),
			},
			{
				label: "Move to the playhead",
				run: () =>
					edit("Move notify", () =>
						rpc("anim.edit", {
							path,
							index: seq,
							op: "notifyMove",
							notify: n.index,
							time: Number(time.toFixed(4)),
						}),
					),
			},
			{ label: "Go to this notify", run: () => seek(n.time) },
			n.notify.ref > 0
				? { label: "Open the notify object", run: () => studio.select(n.notify.ref - 1) }
				: null,
			"-",
			{
				label: "Remove…",
				danger: true,
				run: async () => {
					const ok = await ui.confirm({
						title: `Remove ${notifyName(n)}?`,
						message: `The notify at ${n.time.toFixed(3)} s no longer fires (its object stays in the package). Undo brings it back.`,
						confirm: "Remove",
						danger: true,
					});
					if (ok !== true) return;
					await edit("Remove notify", () =>
						rpc("anim.edit", { path, index: seq, op: "notifyRemove", notify: n.index }),
					);
				},
			},
		]);
	}

	async function applyTrim() {
		if (trimEnd <= trimStart) {
			toasts.push("warning", "The last frame must come after the first.", "Trim");
			return;
		}
		const ok = await ui.confirm({
			title: `Keep frames ${trimStart} to ${trimEnd}?`,
			message: `The sequence becomes ${trimEnd - trimStart + 1} frames (${((trimEnd - trimStart) / fps).toFixed(3)} s); notifies outside are moved to its ends. Undo brings it back.`,
			confirm: "Trim",
		});
		if (ok !== true) return;
		await edit("Trim", () =>
			rpc("anim.edit", { path, index: seq, op: "trim", start: trimStart, end: trimEnd }),
		);
	}

	async function applyLength() {
		if (!(newLength > 0)) return;
		await edit("Retime", () =>
			rpc("anim.edit", { path, index: seq, op: "retime", length: newLength }),
		);
	}

	async function applyRate() {
		if (!(rateScale > 0)) return;
		await edit("Rate scale", () =>
			rpc("object.set", { path, index: seq, prop: "RateScale", value: String(rateScale) }),
		);
	}

	const pct = (t: number) => (length > 0 ? `${(t / length) * 100}%` : "0%");
</script>

<div class="flex h-full min-h-[28rem] flex-col">
	<div class="border-base-300 flex flex-wrap items-center gap-2 border-b px-3 py-1.5 text-xs">
		{#if (info?.sequences.length ?? 0) > 1}
			<select
				class="select select-xs max-w-56"
				bind:value={seq}
				onchange={() => load()}
				title="The AnimSet's sequences"
			>
				{#each info?.sequences ?? [] as q (q.index)}
					<option value={q.index}>{q.name}</option>
				{/each}
			</select>
		{/if}
		<select
			class="select select-xs max-w-72"
			bind:value={meshKey}
			onchange={() => load(true)}
			title="The mesh the animation plays on"
		>
			{#each info?.meshes ?? [] as m (`${m.path}#${m.index}`)}
				<option value="{m.path}#{m.index}"
					>{m.object.split(".").at(-1)} · {m.matched} bones{m.preview
						? " · preview"
						: ""}</option
				>
			{/each}
		</select>
		<label class="flex items-center gap-1"
			><input type="checkbox" class="checkbox checkbox-xs" bind:checked={showSkeleton} /> Skeleton</label
		>
		<button class="btn btn-ghost btn-xs" onclick={fit} title="Frame the model"
			><Focus size={12} /> Fit</button
		>
		<span class="flex-1"></span>
		{#if info}
			<span class="opacity-60"
				>{info.numFrames} frames · {info.length.toFixed(3)} s · {info.fps.toFixed(1)} fps · {info.tracks}
				tracks · {info.keys} keys · {bytes(info.bytes)}</span
			>
		{/if}
	</div>
	<div class="relative min-h-0 flex-1" bind:this={host}>
		<canvas class="block h-full w-full" bind:this={canvas}></canvas>
		{#if loading}
			<div class="absolute inset-0 flex items-center justify-center bg-black/20">
				<span class="loading loading-spinner"></span>
			</div>
		{/if}
		{#if error}
			<div class="alert alert-warning absolute inset-x-3 top-3 text-xs">{error}</div>
		{/if}
		{#if missing.length > 0}
			<div class="absolute bottom-2 left-3 text-[11px] opacity-60" title={missing.join(", ")}>
				{missing.length} tracks have no bone on {meshName}
			</div>
		{/if}
	</div>
	{#if info}
		<div class="border-base-300 flex flex-col gap-1.5 border-t px-3 py-2 text-xs">
			<div class="flex items-center gap-2">
				<button
					class="btn btn-square btn-sm"
					onclick={togglePlay}
					aria-label={playing ? "Pause" : "Play"}
				>
					{#if playing}<Pause size={14} />{:else}<Play size={14} />{/if}
				</button>
				<input
					type="range"
					class="range range-xs flex-1"
					min="0"
					max={Math.max(frames - 1, 0)}
					value={frame}
					oninput={(e) => {
						playing = false;
						seek(Number(e.currentTarget.value) / fps);
					}}
				/>
				<span class="shrink-0 text-right font-mono whitespace-nowrap tabular-nums"
					>{frame} / {frames - 1} · {time.toFixed(2)} s</span
				>
				<select class="select select-xs w-20" bind:value={speed} title="Playback speed">
					{#each [0.25, 0.5, 1, 2] as s (s)}<option value={s}>{s}×</option>{/each}
				</select>
				<button
					class="btn btn-ghost btn-xs {loop ? 'text-primary' : 'opacity-60'}"
					onclick={() => (loop = !loop)}
					title="Loop"
					aria-pressed={loop}><Repeat size={12} /></button
				>
			</div>
			<!-- Notifies: drag to move, right-click for more. -->
			<div
				class="bg-base-200 border-base-300 relative h-7 rounded border"
				bind:this={timeline}
				role="presentation"
				onpointermove={moveDrag}
				onpointerup={endDrag}
				onclick={(e) => {
					if (!dragging && e.target === timeline) {
						playing = false;
						seek(timeAt(e.clientX));
					}
				}}
			>
				<div
					class="bg-primary/60 pointer-events-none absolute inset-y-0 w-0.5"
					style:left={pct(time)}
				></div>
				{#each info.notifies as n (n.index)}
					{@const t = dragging?.index === n.index ? dragging.time : n.time}
					<button
						class="bg-warning absolute top-1 h-5 w-1.5 -translate-x-1/2 cursor-ew-resize rounded-sm hover:scale-125"
						style:left={pct(t)}
						title="{notifyName(n)} ({n.notify.class}) at {t.toFixed(3)} s{n.duration
							? `, ${n.duration.toFixed(2)} s long`
							: ''}: drag to move, right-click for more"
						onpointerdown={(e) => startDrag(e, n)}
						oncontextmenu={(e) => notifyMenu(e, n)}
						aria-label="Notify {notifyName(n)}"
					></button>
				{/each}
			</div>
			<details class="mt-1">
				<summary class="cursor-pointer opacity-70">Edit the sequence</summary>
				<div class="mt-2 grid gap-3 sm:grid-cols-3">
					<div class="flex flex-col gap-1">
						<span class="flex items-center gap-1 opacity-70"
							><Scissors size={12} /> Keep frames</span
						>
						<div class="flex items-center gap-1">
							<input
								class="input input-xs w-16"
								type="number"
								min="0"
								max={frames - 1}
								bind:value={trimStart}
							/>
							<button
								class="btn btn-ghost btn-xs"
								onclick={() => (trimStart = frame)}
								title="From the playhead">⇤</button
							>
							to
							<input
								class="input input-xs w-16"
								type="number"
								min="1"
								max={frames - 1}
								bind:value={trimEnd}
							/>
							<button
								class="btn btn-ghost btn-xs"
								onclick={() => (trimEnd = frame)}
								title="To the playhead">⇥</button
							>
						</div>
						<button class="btn btn-xs w-fit" onclick={applyTrim} disabled={busy}
							>Trim…</button
						>
					</div>
					<div class="flex flex-col gap-1">
						<span class="flex items-center gap-1 opacity-70"
							><Timer size={12} /> Length (s)</span
						>
						<input
							class="input input-xs w-24"
							type="number"
							min="0.01"
							step="0.01"
							bind:value={newLength}
						/>
						<button class="btn btn-xs w-fit" onclick={applyLength} disabled={busy}
							>Retime</button
						>
						<span class="text-[11px] opacity-60"
							>Same frames, faster or slower; notifies follow.</span
						>
					</div>
					<div class="flex flex-col gap-1">
						<span class="opacity-70">Rate scale</span>
						<input
							class="input input-xs w-24"
							type="number"
							min="0.01"
							step="0.05"
							bind:value={rateScale}
						/>
						<button class="btn btn-xs w-fit" onclick={applyRate} disabled={busy}
							>Apply</button
						>
						<span class="text-[11px] opacity-60"
							>How fast the game plays it (1 = as authored).</span
						>
					</div>
				</div>
				<p class="mt-2 text-[11px] opacity-60">
					Replace the motion from Blender or another tool with <b>Replace…</b> (glTF or
					FBX, bones named like the mesh's); <b>Export</b> writes the mesh, its skeleton and
					this animation.
				</p>
			</details>
		</div>
	{/if}
</div>
