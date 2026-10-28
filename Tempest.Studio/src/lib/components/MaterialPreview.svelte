<script lang="ts">
	import { onDestroy } from "svelte";
	import * as THREE from "three";
	import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
	import { blobUrl, rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { MaterialPreview } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let canvas = $state<HTMLCanvasElement | null>(null);
	let host = $state<HTMLDivElement | null>(null);
	let info = $state<MaterialPreview | null>(null);
	let error = $state<string | null>(null);
	let loading = $state(false);
	let shape = $state<"sphere" | "cube" | "plane">("sphere");
	let background = $state<"dark" | "grey" | "light">("dark");
	let thumbs = $state<Record<string, string>>({});

	const BACKGROUNDS = { dark: 0x11111b, grey: 0x585b70, light: 0xcdd6f4 };

	let renderer: THREE.WebGLRenderer | null = null;
	let scene: THREE.Scene | null = null;
	let camera: THREE.PerspectiveCamera | null = null;
	let controls: OrbitControls | null = null;
	let mesh: THREE.Mesh | null = null;
	let material: THREE.MeshStandardMaterial | null = null;
	let frame = 0;
	let resizeObserver: ResizeObserver | null = null;
	let urls: string[] = [];

	function setup() {
		if (!canvas || !host || renderer) return;
		renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
		renderer.setPixelRatio(window.devicePixelRatio);
		renderer.outputColorSpace = THREE.SRGBColorSpace;
		scene = new THREE.Scene();
		scene.background = new THREE.Color(BACKGROUNDS[background]);
		camera = new THREE.PerspectiveCamera(40, 1, 0.1, 100);
		camera.position.set(0, 0.6, 3.2);
		controls = new OrbitControls(camera, canvas);
		controls.enableDamping = true;
		scene.add(new THREE.HemisphereLight(0xffffff, 0x444466, 1.2));
		const key = new THREE.DirectionalLight(0xffffff, 2.2);
		key.position.set(3, 4, 5);
		scene.add(key);
		const fill = new THREE.DirectionalLight(0xffffff, 0.7);
		fill.position.set(-4, 1, -3);
		scene.add(fill);
		material = new THREE.MeshStandardMaterial({
			color: 0x9399b2,
			metalness: 0,
			roughness: 0.6,
		});
		mesh = new THREE.Mesh(geometry(shape), material);
		scene.add(mesh);
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

	function geometry(kind: typeof shape): THREE.BufferGeometry {
		switch (kind) {
			case "cube": {
				return new THREE.BoxGeometry(1.6, 1.6, 1.6);
			}
			case "plane": {
				return new THREE.PlaneGeometry(2.2, 2.2);
			}
			default: {
				return new THREE.SphereGeometry(1, 96, 64);
			}
		}
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

	function loadTexture(bytes: Uint8Array, srgb: boolean): Promise<THREE.Texture> {
		const url = blobUrl(bytes, "image/png");
		urls.push(url);
		return new Promise((resolve, reject) => {
			new THREE.TextureLoader().load(
				url,
				(t) => {
					t.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
					t.wrapS = THREE.RepeatWrapping;
					t.wrapT = THREE.RepeatWrapping;
					t.flipY = false;
					resolve(t);
				},
				undefined,
				reject,
			);
		});
	}

	async function load() {
		loading = true;
		error = null;
		try {
			const r = await rpc("material.preview", { path, index, textureSize: 1024 });
			info = r;
			setup();
			if (!material) return;
			for (const u of urls) URL.revokeObjectURL(u);
			urls = [];
			const next: Record<string, string> = {};
			material.map = null;
			material.normalMap = null;
			material.emissiveMap = null;
			material.emissive.set(0x000000);
			for (const t of r.textures) {
				if (!t.png) continue;
				next[t.path] = blobUrl(t.png, "image/png");
				urls.push(next[t.path]);
				if (t.slot === "basecolor" && !material.map) {
					material.map = await loadTexture(t.png, true);
				} else if (t.slot === "normal" && !material.normalMap) {
					material.normalMap = await loadTexture(t.png, false);
				} else if (t.slot === "emissive" && !material.emissiveMap) {
					material.emissiveMap = await loadTexture(t.png, true);
					material.emissive.set(0xffffff);
				}
			}
			thumbs = next;
			material.color.set(material.map ? 0xffffff : 0x9399b2);
			material.needsUpdate = true;
		} catch (error_) {
			error = error_ instanceof Error ? error_.message : String(error_);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void [path, index];
		void load();
	});
	$effect(() => {
		const kind = shape;
		if (mesh) {
			mesh.geometry.dispose();
			mesh.geometry = geometry(kind);
		}
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
		for (const u of urls) URL.revokeObjectURL(u);
	});

	async function goTo(pkg: string, exportIndex: number) {
		const target = studio.packages.find((p) => p.name.toLowerCase() === pkg.toLowerCase());
		if (!target) return;
		await (studio.pkg?.path === target.path
			? studio.select(exportIndex)
			: studio.openPackage(target.path, exportIndex));
	}

	async function goToObject(pkg: string, object: string) {
		const target = studio.packages.find((p) => p.name.toLowerCase() === pkg.toLowerCase());
		if (!target) return;
		if (studio.pkg?.path !== target.path) await studio.openPackage(target.path);
		const row = studio.pkg?.exports.find((e) => e.path.toLowerCase() === object.toLowerCase());
		if (row) await studio.select(row.i);
	}

	let saving = $state<string | null>(null);

	async function setParameter(
		name: string,
		value: { scalar?: number; vector?: [number, number, number, number] },
	) {
		saving = name;
		try {
			const r = await rpc("material.setParameter", {
				path,
				index,
				name,
				...value,
				textureSize: 1024,
			});
			info = r;
			studio.markDirty(path);
		} catch (error) {
			toasts.error(error, `Could not set ${name}`);
		} finally {
			saving = null;
		}
	}

	function commitScalar(name: string, event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const v = Number.parseFloat(input.value);
		if (!Number.isFinite(v)) return;
		const current = info?.scalars.find((s) => s.name === name)?.value;
		if (current !== undefined && Math.abs(current - v) < 1e-9) return;
		void setParameter(name, { scalar: v });
	}

	function commitVectorComponent(name: string, k: number, event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const v = Number.parseFloat(input.value);
		const current = info?.vectors.find((s) => s.name === name)?.value;
		if (!Number.isFinite(v) || !current || Math.abs(current[k] - v) < 1e-9) return;
		const next = [...current] as [number, number, number, number];
		next[k] = v;
		void setParameter(name, { vector: next });
	}

	function commitColor(name: string, event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const hex = input.value;
		const current = info?.vectors.find((s) => s.name === name)?.value;
		if (!current || !/^#[0-9a-f]{6}$/i.test(hex)) return;
		const next: [number, number, number, number] = [
			Number.parseInt(hex.slice(1, 3), 16) / 255,
			Number.parseInt(hex.slice(3, 5), 16) / 255,
			Number.parseInt(hex.slice(5, 7), 16) / 255,
			current[3],
		];
		void setParameter(name, { vector: next });
	}

	const hex = (v: [number, number, number, number]) =>
		`#${v
			.slice(0, 3)
			.map((x) =>
				Math.round(Math.min(1, Math.max(0, x)) * 255)
					.toString(16)
					.padStart(2, "0"),
			)
			.join("")}`;

	const editable = $derived(studio.selectedRow !== null);

	const rgba = (v: [number, number, number, number]) =>
		`rgb(${Math.round(Math.min(1, Math.max(0, v[0])) * 255)} ${Math.round(Math.min(1, Math.max(0, v[1])) * 255)} ${Math.round(Math.min(1, Math.max(0, v[2])) * 255)})`;
</script>

<div class="flex h-full min-h-0">
	<div class="flex min-w-0 flex-1 flex-col">
		<div class="border-base-300 flex flex-wrap items-center gap-2 border-b px-3 py-1.5 text-xs">
			{#if info}
				<span class="font-mono opacity-70">
					{info.textures.length} textures · {info.scalars.length} scalars · {info.vectors
						.length} vectors
				</span>
			{/if}
			<div class="flex-1"></div>
			<select class="select select-xs w-20" bind:value={shape}>
				<option value="sphere">sphere</option>
				<option value="cube">cube</option>
				<option value="plane">plane</option>
			</select>
			<select class="select select-xs w-20" bind:value={background}>
				<option value="dark">dark</option>
				<option value="grey">grey</option>
				<option value="light">light</option>
			</select>
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
				base colour, normal and emissive maps only; the game's master material does more
			</div>
		</div>
	</div>
	{#if info}
		<div class="border-base-300 w-80 shrink-0 overflow-y-auto border-l text-xs">
			<div class="px-3 py-1 text-[10px] opacity-60">
				Scalars and vectors are editable: on an instance the value becomes an override of
				that instance; on a material it changes the parameter's default. Save the package to
				keep it.
			</div>
			<div class="pane-title">Parent chain</div>
			<div class="space-y-0.5 px-3 py-1 font-mono">
				{#each info.chain as c, k (k)}
					<button
						class="link link-primary block truncate text-left"
						onclick={() => goToObject(c.package, c.object)}
						title="{c.class} · {c.package}.{c.object}"
					>
						{"  ".repeat(k)}{c.object}
					</button>
				{/each}
			</div>
			<div class="pane-title">Textures</div>
			<div class="px-2 py-1">
				{#each info.textures as t, i (`${i}:${t.path}`)}
					<button
						class="row-hover flex w-full items-center gap-2 rounded p-1 text-left"
						onclick={() => goTo(t.package, t.export)}
						title={t.path}
					>
						{#if thumbs[t.path]}
							<img
								src={thumbs[t.path]}
								alt=""
								class="checker h-10 w-10 shrink-0 rounded object-cover"
							/>
						{:else}
							<div class="bg-base-300 h-10 w-10 shrink-0 rounded"></div>
						{/if}
						<div class="min-w-0">
							<div class="truncate font-mono">
								{t.parameter} <span class="opacity-50">{t.slot}</span>
							</div>
							<div class="truncate font-mono text-[10px] opacity-60">
								{t.path.split(".").at(-1)}
							</div>
							{#if t.error}<div class="text-error">{t.error}</div>{/if}
						</div>
					</button>
				{/each}
			</div>
			{#if info.vectors.length}
				<div class="pane-title">Vectors</div>
				<table class="table-xs table w-full table-fixed font-mono">
					<tbody>
						{#each info.vectors as v, i (`${i}:${v.name}`)}
							<tr class={saving === v.name ? "opacity-50" : ""}>
								<td class="w-6">
									<input
										type="color"
										class="h-4 w-4 cursor-pointer border-0 bg-transparent p-0"
										value={hex(v.value)}
										onchange={(e) => commitColor(v.name, e)}
										disabled={!editable}
										title="pick RGB (alpha and values above 1 stay as typed)"
									/>
								</td>
								<td class="truncate" title={v.name}>{v.name}</td>
								<td class="w-44">
									<div class="flex gap-0.5">
										{#each [0, 1, 2, 3] as k (k)}
											<input
												class="input input-xs w-10 px-1 text-right font-mono"
												type="number"
												step="0.01"
												value={+v.value[k]!.toFixed(4)}
												onchange={(e) =>
													commitVectorComponent(v.name, k, e)}
												disabled={!editable}
											/>
										{/each}
									</div>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			{/if}
			{#if info.scalars.length}
				<div class="pane-title">Scalars</div>
				<table class="table-xs table w-full table-fixed font-mono">
					<tbody>
						{#each info.scalars as sc, i (`${i}:${sc.name}`)}
							<tr class={saving === sc.name ? "opacity-50" : ""}>
								<td class="truncate" title={sc.name}>{sc.name}</td>
								<td class="w-24">
									<input
										class="input input-xs w-full text-right font-mono"
										type="number"
										step="0.01"
										value={+sc.value.toFixed(4)}
										onchange={(e) => commitScalar(sc.name, e)}
										disabled={!editable}
									/>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			{/if}
			{#if info.switches.length}
				<div class="pane-title">Static switches</div>
				<table class="table-xs table font-mono">
					<tbody>
						{#each info.switches as s, i (`${i}:${s.name}`)}
							<tr
								><td class="truncate">{s.name}</td><td class="text-right opacity-70"
									>{s.value ? "on" : "off"}</td
								></tr
							>
						{/each}
					</tbody>
				</table>
			{/if}
		</div>
	{/if}
</div>

<style>
	.checker {
		background-color: #808080;
		background-image:
			linear-gradient(45deg, #666 25%, transparent 25%),
			linear-gradient(-45deg, #666 25%, transparent 25%),
			linear-gradient(45deg, transparent 75%, #666 75%),
			linear-gradient(-45deg, transparent 75%, #666 75%);
		background-size: 8px 8px;
		background-position:
			0 0,
			0 4px,
			4px -4px,
			-4px 0;
	}
</style>
