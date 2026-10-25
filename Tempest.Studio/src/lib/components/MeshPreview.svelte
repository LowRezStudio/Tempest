<script lang="ts">
	import { Focus } from "@lucide/svelte";
	import { onDestroy, untrack } from "svelte";
	import * as THREE from "three";
	import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
	import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
	import { bytes } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import type { MeshInfo } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let canvas = $state<HTMLCanvasElement | null>(null);
	let host = $state<HTMLDivElement | null>(null);
	let info = $state<MeshInfo | null>(null);
	let error = $state<string | null>(null);
	let loading = $state(false);
	let lod = $state(0);
	let textures = $state(true);
	let wireframe = $state(false);
	let showSkeleton = $state(false);
	let showGrid = $state(true);
	let background = $state<"dark" | "grey" | "light">("dark");
	let stats = $state<{
		vertices: number;
		triangles: number;
		materials: number;
		images: number;
		bytes: number;
	} | null>(null);

	const BACKGROUNDS = { dark: 0x11111b, grey: 0x585b70, light: 0xcdd6f4 };

	let renderer: THREE.WebGLRenderer | null = null;
	let scene: THREE.Scene | null = null;
	let camera: THREE.PerspectiveCamera | null = null;
	let controls: OrbitControls | null = null;
	let model: THREE.Group | null = null;
	let skeletonHelper: THREE.SkeletonHelper | null = null;
	let grid: THREE.GridHelper | null = null;
	let frame = 0;
	let resizeObserver: ResizeObserver | null = null;

	function setup() {
		if (!canvas || !host || renderer) return;
		renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
		renderer.setPixelRatio(window.devicePixelRatio);
		renderer.outputColorSpace = THREE.SRGBColorSpace;
		scene = new THREE.Scene();
		scene.background = new THREE.Color(BACKGROUNDS[background]);
		camera = new THREE.PerspectiveCamera(45, 1, 0.01, 1000);
		camera.position.set(2, 1.5, 2);
		controls = new OrbitControls(camera, canvas);
		controls.enableDamping = true;
		scene.add(new THREE.HemisphereLight(0xffffff, 0x444466, 1.4));
		const key = new THREE.DirectionalLight(0xffffff, 2.0);
		key.position.set(3, 5, 4);
		scene.add(key);
		const fill = new THREE.DirectionalLight(0xffffff, 0.8);
		fill.position.set(-4, 2, -3);
		scene.add(fill);
		grid = new THREE.GridHelper(4, 16, 0x6c7086, 0x45475a);
		grid.visible = showGrid;
		scene.add(grid);
		resizeObserver = new ResizeObserver(resize);
		resizeObserver.observe(host);
		resize();
		const loop = () => {
			frame = requestAnimationFrame(loop);
			controls?.update();
			if (renderer && scene && camera) renderer.render(scene, camera);
		};
		loop();
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

	function applyWireframe() {
		model?.traverse((o) => {
			if (o instanceof THREE.Mesh) {
				const mats = Array.isArray(o.material) ? o.material : [o.material];
				for (const m of mats) {
					if ("wireframe" in m) (m as THREE.MeshStandardMaterial).wireframe = wireframe;
				}
			}
		});
	}

	/** Frame the model: the grid sits under it, the camera looks at its centre. */
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
			.add(new THREE.Vector3(radius * 1.1, radius * 0.8, radius * 1.1));
		camera.updateProjectionMatrix();
		controls.update();
		grid.position.set(center.x, box.min.y, center.z);
		const extent = Math.max(size.x, size.z) * 2;
		grid.scale.setScalar(extent / 4);
	}

	async function load() {
		loading = true;
		error = null;
		try {
			const current = untrack(() => info);
			if (!current) info = await rpc("mesh.info", { path, index });
			const r = await rpc("mesh.preview", { path, index, lod, textures, skin: true });
			stats = {
				vertices: r.vertices,
				triangles: r.triangles,
				materials: r.materials,
				images: r.images,
				bytes: r.bytes,
			};
			const glb = r.glb ?? new Uint8Array();
			const gltf = await new GLTFLoader().parseAsync(
				glb.buffer.slice(glb.byteOffset, glb.byteOffset + glb.byteLength) as ArrayBuffer,
				"",
			);
			setup();
			if (!scene) return;
			if (model) scene.remove(model);
			if (skeletonHelper) scene.remove(skeletonHelper);
			model = gltf.scene;
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
			scene.add(model);
			skeletonHelper = new THREE.SkeletonHelper(model);
			skeletonHelper.visible = showSkeleton;
			scene.add(skeletonHelper);
			applyWireframe();
			fit();
		} catch (error_) {
			error = error_ instanceof Error ? error_.message : String(error_);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		// Re-export when the LOD or texture choice changes.
		void [lod, textures];
		void load();
	});
	$effect(() => {
		const on = wireframe;
		model?.traverse((o) => {
			if (o instanceof THREE.Mesh) {
				const mats = Array.isArray(o.material) ? o.material : [o.material];
				for (const m of mats) {
					if ("wireframe" in m) (m as THREE.MeshStandardMaterial).wireframe = on;
				}
			}
		});
	});
	// Read the state first: an effect only tracks what it actually reads, and
	// the three.js objects do not exist yet on the first run.
	$effect(() => {
		const visible = showSkeleton;
		if (skeletonHelper) skeletonHelper.visible = visible;
	});
	$effect(() => {
		const visible = showGrid;
		if (grid) grid.visible = visible;
	});
	$effect(() => {
		const color = BACKGROUNDS[background];
		if (scene) scene.background = new THREE.Color(color);
	});

	onDestroy(() => {
		cancelAnimationFrame(frame);
		resizeObserver?.disconnect();
		controls?.dispose();
		renderer?.dispose();
	});

	const bones = $derived.by(() => {
		const l = info?.lods[lod];
		return l && "bones" in l ? l.bones : 0;
	});
</script>

<div class="flex h-full flex-col">
	<div class="border-base-300 flex flex-wrap items-center gap-2 border-b px-3 py-1.5 text-xs">
		{#if info}
			<span class="badge badge-ghost badge-sm">{info.skeletal ? "skeletal" : "static"}</span>
			{#if stats}
				<span class="font-mono opacity-70"
					>{stats.vertices} verts · {stats.triangles} tris · {stats.materials} materials · {stats.images}
					textures{bones ? ` · ${bones} bones` : ""}</span
				>
			{/if}
			<div class="flex-1"></div>
			<label class="flex items-center gap-1">
				LOD
				<select class="select select-xs w-16" bind:value={lod}>
					{#each info.lods as l (l.index)}
						<option value={l.index}>{l.index}</option>
					{/each}
				</select>
			</label>
			<label class="flex items-center gap-1"
				><input type="checkbox" class="checkbox checkbox-xs" bind:checked={textures} /> textures</label
			>
			<label class="flex items-center gap-1"
				><input type="checkbox" class="checkbox checkbox-xs" bind:checked={wireframe} /> wireframe</label
			>
			{#if info.skeletal}
				<label class="flex items-center gap-1"
					><input
						type="checkbox"
						class="checkbox checkbox-xs"
						bind:checked={showSkeleton}
					/> skeleton</label
				>
			{/if}
			<label class="flex items-center gap-1"
				><input type="checkbox" class="checkbox checkbox-xs" bind:checked={showGrid} /> grid</label
			>
			<select class="select select-xs w-20" bind:value={background}>
				<option value="dark">dark</option>
				<option value="grey">grey</option>
				<option value="light">light</option>
			</select>
			<button class="btn btn-ghost btn-xs" onclick={fit} title="Frame the model">
				<Focus size={12} /> Fit
			</button>
		{/if}
	</div>
	<div class="relative min-h-0 flex-1" bind:this={host}>
		<canvas bind:this={canvas} class="block h-full w-full"></canvas>
		{#if loading}
			<div class="bg-base-100/40 absolute inset-0 flex items-center justify-center">
				<span class="loading loading-spinner"></span>
			</div>
		{/if}
		{#if error}
			<div class="bg-error/80 text-error-content absolute inset-x-0 bottom-0 p-2 text-xs">
				{error}
			</div>
		{/if}
		<div class="absolute right-2 bottom-2 text-[10px] opacity-40">
			drag: orbit · right-drag: pan · wheel: zoom
		</div>
	</div>
	{#if info}
		<div
			class="border-base-300 max-h-24 shrink-0 overflow-auto border-t px-3 py-1 font-mono text-[11px]"
		>
			{#each info.lods as l (l.index)}
				{#if "error" in l}
					<div class="text-error">LOD {l.index}: {l.error}</div>
				{:else}
					<div>
						LOD {l.index}: {l.vertices} vertices, {l.triangles} triangles —
						{#each l.sections as s, k (k)}
							<span class="opacity-80"
								>{s.material.path || "no material"} ({s.triangles})</span
							>{k < l.sections.length - 1 ? ", " : ""}
						{/each}
					</div>
				{/if}
			{/each}
			{#if stats}<div class="opacity-50">preview glb {bytes(stats.bytes)}</div>{/if}
		</div>
	{/if}
</div>
