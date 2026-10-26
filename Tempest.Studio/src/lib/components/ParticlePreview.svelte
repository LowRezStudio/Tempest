<script lang="ts">
	import { Focus, Pause, Play, RotateCcw } from "@lucide/svelte";
	import { onDestroy } from "svelte";
	import * as THREE from "three";
	import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
	import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
	import CurveEditor from "$lib/components/CurveEditor.svelte";
	import {
		buildEmitter,
		decodeDist,
		distTable,
		EmitterSim,
		orbitOffset,
		propOf,
		type Dist,
		type EmitterModel,
		type SystemJson,
	} from "$lib/studio/particles";
	import { blobUrl, rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { ParsedObject, Prop } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let canvas = $state<HTMLCanvasElement | null>(null);
	let host = $state<HTMLDivElement | null>(null);
	let json = $state<SystemJson | null>(null);
	let error = $state<string | null>(null);
	let loading = $state(false);
	let playing = $state(true);
	let loop = $state(true);
	let speed = $state(1);
	let showGrid = $state(true);
	let background = $state<"dark" | "grey" | "light">("dark");
	let visible = $state<Record<number, boolean>>({});
	let selectedEmitter = $state<number | null>(null);
	let selectedModule = $state<number | null>(null);
	let counts = $state<Record<number, number>>({});
	let frameNo = $state(0);

	const BACKGROUNDS = { dark: 0x11111b, grey: 0x585b70, light: 0xcdd6f4 };
	const SCALE = 0.01;

	let renderer: THREE.WebGLRenderer | null = null;
	let scene: THREE.Scene | null = null;
	let camera: THREE.PerspectiveCamera | null = null;
	let controls: OrbitControls | null = null;
	let grid: THREE.GridHelper | null = null;
	let frame = 0;
	let last = 0;
	let resizeObserver: ResizeObserver | null = null;
	let textures: Record<string, THREE.Texture | null> = {};
	let meshes: Record<string, { geometry: THREE.BufferGeometry; material: THREE.Material }[]> = {};

	interface SpriteView {
		kind: "sprite";
		mesh: THREE.Mesh;
		geometry: THREE.InstancedBufferGeometry;
		pos: THREE.InstancedBufferAttribute;
		size: THREE.InstancedBufferAttribute;
		rot: THREE.InstancedBufferAttribute;
		color: THREE.InstancedBufferAttribute;
		sub: THREE.InstancedBufferAttribute;
		cap: number;
	}
	interface MeshView {
		kind: "mesh";
		instances: THREE.InstancedMesh[];
		cap: number;
	}
	interface Runtime {
		model: EmitterModel;
		sim: EmitterSim;
		view: SpriteView | MeshView | null;
	}
	let runtimes: Runtime[] = [];
	/** The models, for the side panel (runtimes are not reactive). */
	let models = $state<EmitterModel[]>([]);

	const VERT = `
		attribute vec3 iPos;
		attribute vec3 iSize;
		attribute float iRot;
		attribute vec4 iColor;
		attribute float iSub;
		uniform vec2 subImages;
		uniform int alignMode;
		varying vec2 vUv;
		varying vec4 vColor;
		void main() {
			vec2 corner = position.xy;
			float c = cos(iRot);
			float s = sin(iRot);
			vec2 r = vec2(corner.x * c - corner.y * s, corner.x * s + corner.y * c) * iSize.xy;
			vec4 mv;
			if (alignMode == 0) {
				mv = modelViewMatrix * vec4(iPos, 1.0);
				mv.xy += r;
			} else {
				vec3 world;
				if (alignMode == 1) world = iPos + vec3(r.x, 0.0, -r.y);
				else if (alignMode == 2) world = iPos + vec3(0.0, r.y, r.x);
				else world = iPos + vec3(r.x, r.y, 0.0);
				mv = modelViewMatrix * vec4(world, 1.0);
			}
			gl_Position = projectionMatrix * mv;
			float idx = floor(iSub + 0.5);
			float col = mod(idx, subImages.x);
			float row = floor(idx / subImages.x);
			vUv = (uv + vec2(col, subImages.y - 1.0 - row)) / subImages;
			vColor = iColor;
		}`;
	const FRAG = `
		uniform sampler2D map;
		uniform int hasMap;
		varying vec2 vUv;
		varying vec4 vColor;
		void main() {
			vec4 t = hasMap == 1 ? texture2D(map, vUv) : vec4(1.0);
			gl_FragColor = t * vColor;
			if (gl_FragColor.a < 0.002) discard;
		}`;

	function setup() {
		if (!canvas || !host || renderer) return;
		renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
		renderer.setPixelRatio(window.devicePixelRatio);
		renderer.outputColorSpace = THREE.SRGBColorSpace;
		scene = new THREE.Scene();
		scene.background = new THREE.Color(BACKGROUNDS[background]);
		camera = new THREE.PerspectiveCamera(45, 1, 0.01, 1000);
		camera.position.set(3, 2, 3);
		controls = new OrbitControls(camera, canvas);
		controls.enableDamping = true;
		scene.add(new THREE.HemisphereLight(0xffffff, 0x444466, 1.2));
		const key = new THREE.DirectionalLight(0xffffff, 1.5);
		key.position.set(3, 5, 4);
		scene.add(key);
		grid = new THREE.GridHelper(4, 16, 0x6c7086, 0x45475a);
		grid.visible = showGrid;
		scene.add(grid);
		const axes = new THREE.AxesHelper(0.5);
		scene.add(axes);
		resizeObserver = new ResizeObserver(resize);
		resizeObserver.observe(host);
		resize();
		last = performance.now();
		const loop = (now: number) => {
			frame = requestAnimationFrame(loop);
			const dt = Math.min(0.05, (now - last) / 1000) * speed;
			last = now;
			if (playing) tick(dt);
			controls?.update();
			if (renderer && scene && camera) renderer.render(scene, camera);
		};
		frame = requestAnimationFrame(loop);
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

	function blendingOf(mode: string | null): {
		blending: THREE.Blending;
		depthWrite: boolean;
		alphaTest: number;
	} {
		const m = (mode ?? "").toLowerCase();
		if (m.includes("additive")) {
			return { blending: THREE.AdditiveBlending, depthWrite: false, alphaTest: 0 };
		}
		if (m.includes("modulate")) {
			return { blending: THREE.MultiplyBlending, depthWrite: false, alphaTest: 0 };
		}
		if (m.includes("masked")) {
			return { blending: THREE.NormalBlending, depthWrite: true, alphaTest: 0.33 };
		}
		if (m.includes("opaque")) {
			return { blending: THREE.NoBlending, depthWrite: true, alphaTest: 0 };
		}
		return { blending: THREE.NormalBlending, depthWrite: false, alphaTest: 0 };
	}

	async function textureOf(materialPath: string | null): Promise<THREE.Texture | null> {
		if (!materialPath || !json) return null;
		if (materialPath in textures) return textures[materialPath];
		const m = json.materials[materialPath];
		let tex: THREE.Texture | null = null;
		if (m?.png) {
			const url = blobUrl(m.png, "image/png");
			try {
				tex = await new THREE.TextureLoader().loadAsync(url);
				tex.colorSpace = THREE.SRGBColorSpace;
			} finally {
				URL.revokeObjectURL(url);
			}
		}
		textures[materialPath] = tex;
		return tex;
	}

	async function meshOf(
		meshPath: string,
	): Promise<{ geometry: THREE.BufferGeometry; material: THREE.Material }[]> {
		if (meshPath in meshes) return meshes[meshPath];
		const where = json?.meshes[meshPath];
		let parts: { geometry: THREE.BufferGeometry; material: THREE.Material }[] = [];
		if (where?.path) {
			try {
				const r = await rpc("mesh.preview", {
					path: where.path,
					index: where.index,
					textures: true,
					skin: false,
					textureSize: 512,
				});
				const glb = r.glb ?? new Uint8Array();
				const gltf = await new GLTFLoader().parseAsync(
					glb.buffer.slice(
						glb.byteOffset,
						glb.byteOffset + glb.byteLength,
					) as ArrayBuffer,
					"",
				);
				gltf.scene.traverse((o) => {
					if (o instanceof THREE.Mesh) {
						const mats = Array.isArray(o.material) ? o.material : [o.material];
						for (const mat of mats) {
							if (mat instanceof THREE.MeshStandardMaterial) {
								mat.side = THREE.DoubleSide;
								mat.metalness = 0;
							}
						}
						parts.push({ geometry: o.geometry, material: mats[0] });
					}
				});
			} catch (error_) {
				toasts.error(error_, `Mesh ${meshPath}`);
				parts = [];
			}
		}
		meshes[meshPath] = parts;
		return parts;
	}

	function alignMode(m: EmitterModel): number {
		const a = m.axisLock;
		if (a.includes("_z")) return 1;
		if (a.includes("_x")) return 2;
		if (a.includes("_y")) return 3;
		return 0;
	}

	async function buildView(model: EmitterModel): Promise<SpriteView | MeshView | null> {
		if (!scene) return null;
		const cap = Math.max(256, model.peak * 4);
		const material = model.material ? json?.materials[model.material] : undefined;
		if (model.meshPath) {
			const parts = await meshOf(model.meshPath);
			if (parts.length === 0) return null;
			const override = model.overrideMaterial ? await textureOf(model.material) : null;
			const instances = parts.map((p) => {
				let mat = p.material;
				if (override) {
					const b = blendingOf(material?.blendMode ?? null);
					mat = new THREE.MeshBasicMaterial({
						map: override,
						transparent: b.blending !== THREE.NoBlending,
						blending: b.blending,
						depthWrite: b.depthWrite,
						side: THREE.DoubleSide,
					});
				}
				const im = new THREE.InstancedMesh(p.geometry, mat, cap);
				im.count = 0;
				im.frustumCulled = false;
				scene?.add(im);
				return im;
			});
			return { kind: "mesh", instances, cap };
		}
		const tex = await textureOf(model.material);
		const b = blendingOf(material?.blendMode ?? null);
		const geometry = new THREE.InstancedBufferGeometry();
		const plane = new THREE.PlaneGeometry(1, 1);
		geometry.index = plane.index;
		geometry.setAttribute("position", plane.getAttribute("position"));
		geometry.setAttribute("uv", plane.getAttribute("uv"));
		const pos = new THREE.InstancedBufferAttribute(new Float32Array(cap * 3), 3);
		const size = new THREE.InstancedBufferAttribute(new Float32Array(cap * 3), 3);
		const rot = new THREE.InstancedBufferAttribute(new Float32Array(cap), 1);
		const color = new THREE.InstancedBufferAttribute(new Float32Array(cap * 4), 4);
		const sub = new THREE.InstancedBufferAttribute(new Float32Array(cap), 1);
		for (const a of [pos, size, rot, color, sub]) a.setUsage(THREE.DynamicDrawUsage);
		geometry.setAttribute("iPos", pos);
		geometry.setAttribute("iSize", size);
		geometry.setAttribute("iRot", rot);
		geometry.setAttribute("iColor", color);
		geometry.setAttribute("iSub", sub);
		geometry.instanceCount = 0;
		const shader = new THREE.ShaderMaterial({
			vertexShader: VERT,
			fragmentShader: FRAG,
			uniforms: {
				map: { value: tex },
				hasMap: { value: tex ? 1 : 0 },
				subImages: { value: new THREE.Vector2(model.subH, model.subV) },
				alignMode: { value: alignMode(model) },
			},
			transparent: b.blending !== THREE.NoBlending,
			blending: b.blending,
			depthWrite: b.depthWrite,
			alphaTest: b.alphaTest,
			side: THREE.DoubleSide,
		});
		const mesh = new THREE.Mesh(geometry, shader);
		mesh.frustumCulled = false;
		scene.add(mesh);
		return { kind: "sprite", mesh, geometry, pos, size, rot, color, sub, cap };
	}

	function disposeViews() {
		for (const r of runtimes) {
			if (!r.view) continue;
			if (r.view.kind === "sprite") {
				scene?.remove(r.view.mesh);
				r.view.geometry.dispose();
				(r.view.mesh.material as THREE.Material).dispose();
			} else {
				for (const im of r.view.instances) {
					scene?.remove(im);
					im.dispose();
				}
			}
		}
		runtimes = [];
	}

	const tmpM = new THREE.Matrix4();
	const tmpQ = new THREE.Quaternion();
	const tmpE = new THREE.Euler();
	const tmpP = new THREE.Vector3();
	const tmpS = new THREE.Vector3();
	const tmpC = new THREE.Color();
	const orbit: [number, number, number] = [0, 0, 0];

	function tick(dt: number) {
		const next: Record<number, number> = {};
		for (const r of runtimes) {
			const show = visible[r.model.index] !== false;
			if (show) r.sim.step(dt);
			next[r.model.index] = r.sim.particles.length;
			const v = r.view;
			if (!v) continue;
			const ps = show ? r.sim.particles : [];
			const n = Math.min(ps.length, v.cap);
			if (v.kind === "sprite") {
				for (let i = 0; i < n; i++) {
					const p = ps[i];
					orbitOffset(p, orbit);
					v.pos.setXYZ(
						i,
						(p.pos[0] + orbit[0]) * SCALE,
						(p.pos[2] + orbit[2]) * SCALE,
						(p.pos[1] + orbit[1]) * SCALE,
					);
					v.size.setXYZ(i, p.size[0] * SCALE, p.size[1] * SCALE, p.size[2] * SCALE);
					v.rot.setX(i, p.rot);
					v.color.setXYZW(i, p.color[0], p.color[1], p.color[2], p.color[3]);
					v.sub.setX(i, p.sub);
				}
				v.geometry.instanceCount = n;
				for (const a of [v.pos, v.size, v.rot, v.color, v.sub]) a.needsUpdate = true;
				v.mesh.visible = n > 0;
			} else {
				for (const im of v.instances) {
					for (let i = 0; i < n; i++) {
						const p = ps[i];
						orbitOffset(p, orbit);
						tmpP.set(
							(p.pos[0] + orbit[0]) * SCALE,
							(p.pos[2] + orbit[2]) * SCALE,
							(p.pos[1] + orbit[1]) * SCALE,
						);
						tmpE.set(p.meshRot[0], p.meshRot[2], -p.meshRot[1]);
						tmpQ.setFromEuler(tmpE);
						tmpS.set(
							Math.max(0.001, p.size[0]),
							Math.max(0.001, p.size[2]),
							Math.max(0.001, p.size[1]),
						);
						tmpM.compose(tmpP, tmpQ, tmpS);
						im.setMatrixAt(i, tmpM);
						tmpC.setRGB(p.color[0], p.color[1], p.color[2]);
						im.setColorAt(i, tmpC);
					}
					im.count = n;
					im.instanceMatrix.needsUpdate = true;
					if (im.instanceColor) im.instanceColor.needsUpdate = true;
				}
			}
		}
		frameNo++;
		if (frameNo % 10 === 0) counts = next;
		// One-shot effects: start over once everything has played out.
		if (
			loop &&
			runtimes.length > 0 &&
			runtimes.every((r) => r.sim.finished && r.sim.particles.length === 0)
		) {
			restart();
		}
	}

	/** Dev hook for the screenshot harness: advance the simulation by `seconds` in fixed steps. */
	if (import.meta.env.DEV) {
		(window as unknown as { __particleStep?: (seconds: number) => number }).__particleStep = (
			seconds: number,
		) => {
			const steps = Math.max(1, Math.round(seconds / (1 / 60)));
			for (let i = 0; i < steps; i++) tick(1 / 60);
			return runtimes.reduce((n, r) => n + r.sim.particles.length, 0);
		};
	}

	function restart() {
		for (const [k, r] of runtimes.entries()) r.sim.reset(k + 1);
	}

	function fit() {
		if (!camera || !controls || !grid) return;
		const box = new THREE.Box3();
		let any = false;
		for (const r of runtimes) {
			for (const p of r.sim.particles) {
				box.expandByPoint(
					new THREE.Vector3(p.pos[0] * SCALE, p.pos[2] * SCALE, p.pos[1] * SCALE),
				);
				any = true;
			}
		}
		if (!any) {
			box.setFromCenterAndSize(new THREE.Vector3(0, 0.5, 0), new THREE.Vector3(1, 1, 1));
		}
		const size = box.getSize(new THREE.Vector3());
		const center = box.getCenter(new THREE.Vector3());
		const radius = Math.max(size.x, size.y, size.z, 0.5);
		controls.target.copy(center);
		camera.position
			.copy(center)
			.add(new THREE.Vector3(radius * 1.2, radius * 0.9, radius * 1.2));
		camera.near = radius / 100;
		camera.far = radius * 100;
		camera.updateProjectionMatrix();
		controls.update();
		grid.position.set(center.x, Math.min(0, box.min.y), center.z);
		grid.scale.setScalar(Math.max(1, (Math.max(size.x, size.z) * 2) / 4));
	}

	/** Build the runtime of every emitter from the JSON (keeps particles where the emitter still exists). */
	async function rebuild(keep = false) {
		if (!json) return;
		setup();
		const old = keep ? runtimes : [];
		if (!keep) disposeViews();
		const next: Runtime[] = [];
		for (const [k, e] of json.emitters.entries()) {
			const model = buildEmitter(e);
			const prev = old.find((r) => r.model.index === model.index);
			if (prev) {
				prev.model = model;
				prev.sim.model = model;
				next.push(prev);
			} else {
				let view: SpriteView | MeshView | null = null;
				try {
					view = await buildView(model);
				} catch (error_) {
					console.error("particle emitter view", model.name, error_);
					toasts.error(error_, `Emitter ${model.name}`);
				}
				next.push({ model, sim: new EmitterSim(model, k + 1), view });
			}
		}
		runtimes = next;
		models = next.map((r) => r.model);
	}

	async function load() {
		loading = true;
		error = null;
		try {
			json = await rpc("particle.system", { path, index, textureSize: 256 });
			textures = {};
			meshes = {};
			visible = {};
			await rebuild(false);
			// Select what the inspector points at.
			selectedEmitter = null;
			selectedModule = null;
			for (const e of json.emitters) {
				const lod = e.lods[0];
				const mods = [
					lod?.required,
					lod?.spawn,
					lod?.typeData,
					...(lod?.modules ?? []),
				].filter((m): m is ParsedObject => !!m);
				if (
					e.index === index ||
					lod?.index === index ||
					mods.some((m) => m.index === index)
				) {
					selectedEmitter = e.index;
					if (mods.some((m) => m.index === index)) selectedModule = index;
				}
			}
			selectedEmitter ??= json.emitters[0]?.index ?? null;
			setTimeout(fit, 600);
		} catch (error_) {
			error = error_ instanceof Error ? error_.message : String(error_);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void load();
	});
	$effect(() => {
		const v = showGrid;
		if (grid) grid.visible = v;
	});
	$effect(() => {
		const c = BACKGROUNDS[background];
		if (scene) scene.background = new THREE.Color(c);
	});

	onDestroy(() => {
		cancelAnimationFrame(frame);
		resizeObserver?.disconnect();
		disposeViews();
		for (const t of Object.values(textures)) t?.dispose();
		controls?.dispose();
		renderer?.dispose();
	});

	// ----- side panel -----------------------------------------------------------

	const emitter = $derived(json?.emitters.find((e) => e.index === selectedEmitter) ?? null);
	const moduleList = $derived.by(() => {
		const lod = emitter?.lods[0];
		if (!lod) return [] as { obj: ParsedObject; role: string }[];
		const out: { obj: ParsedObject; role: string }[] = [];
		if (lod.required) out.push({ obj: lod.required, role: "required" });
		if (lod.spawn) out.push({ obj: lod.spawn, role: "spawn" });
		if (lod.typeData) out.push({ obj: lod.typeData, role: "type data" });
		for (const m of lod.modules) out.push({ obj: m, role: "" });
		return out;
	});
	const module = $derived(moduleList.find((m) => m.obj.index === selectedModule)?.obj ?? null);
	const moduleDists = $derived.by(() => {
		if (!module || module.kind !== "object") return [] as Dist[];
		const out: Dist[] = [];
		for (const p of module.properties) {
			if (
				p.value.t === "struct" &&
				(p.struct ?? "").toLowerCase().startsWith("rawdistribution")
			) {
				out.push(
					decodeDist(
						p.name,
						(p.struct ?? "").toLowerCase().includes("vector") ? "vector" : "float",
						p.value.v,
					),
				);
			}
		}
		return out;
	});
	const modelOf = (e: number) => models.find((m) => m.index === e);
	const shortClass = (c: string) =>
		c.replace(/^particlemodule/, "").replace(/^typedata/, "type: ") || c;

	/** Put a (draft) distribution into the module's JSON and rebuild the live model. */
	function patchDist(d: Dist, persisted?: ParsedObject) {
		if (!json || !module) return;
		const replace = (obj: ParsedObject): ParsedObject => {
			if (persisted) return persisted;
			if (obj.kind !== "object") return obj;
			const properties = obj.properties.map((p): Prop => {
				if (p.name !== d.prop || p.value.t !== "struct") return p;
				const members = p.value.v.filter(
					(m) =>
						!["lookuptable", "lookuptabletimescale", "lookuptablestarttime"].includes(
							m.name.toLowerCase(),
						),
				);
				members.push(
					{
						name: "lookuptable",
						index: 0,
						type: "ArrayProperty",
						value: { t: "array", v: distTable(d).map((x) => ({ t: "float", v: x })) },
					},
					{
						name: "lookuptabletimescale",
						index: 0,
						type: "FloatProperty",
						value: { t: "float", v: d.timeScale },
					},
					{
						name: "lookuptablestarttime",
						index: 0,
						type: "FloatProperty",
						value: { t: "float", v: d.startTime },
					},
				);
				return { ...p, value: { t: "struct", v: members } };
			});
			return { ...obj, properties };
		};
		const target = module.index;
		json = {
			...json,
			emitters: json.emitters.map((e) => ({
				...e,
				lods: e.lods.map((l) => ({
					...l,
					required: l.required?.index === target ? replace(l.required) : l.required,
					spawn: l.spawn?.index === target ? replace(l.spawn) : l.spawn,
					typeData: l.typeData?.index === target ? replace(l.typeData) : l.typeData,
					modules: l.modules.map((m) => (m.index === target ? replace(m) : m)),
				})),
			})),
		};
		void rebuild(true);
	}

	async function applyDist(d: Dist) {
		if (!module) return;
		try {
			const r = await rpc("particle.setTable", {
				path,
				index: module.index,
				prop: d.prop,
				table: distTable(d),
				timeScale: d.timeScale,
				startTime: d.startTime,
			});
			patchDist(d, r);
			studio.markDirty(path);
			toasts.push(
				"success",
				`${d.prop} of ${module.path.split(".").pop()} updated. Save the package to write it.`,
				"Particle edited",
			);
		} catch (error_) {
			toasts.error(error_, "Not applied");
		}
	}

	const total = $derived(Object.values(counts).reduce((a, b) => a + b, 0));
</script>

<div class="flex h-full flex-col">
	<div class="border-base-300 flex flex-wrap items-center gap-2 border-b px-3 py-1.5 text-xs">
		{#if json}
			<span class="badge badge-ghost badge-sm">{json.emitters.length} emitters</span>
			<span class="font-mono opacity-70">{total} particles</span>
			<div class="flex-1"></div>
			<button class="btn btn-ghost btn-xs" onclick={() => (playing = !playing)}>
				{#if playing}<Pause size={12} /> Pause{:else}<Play size={12} /> Play{/if}
			</button>
			<button class="btn btn-ghost btn-xs" onclick={restart} title="Restart every emitter"
				><RotateCcw size={12} /> Restart</button
			>
			<select class="select select-xs w-16" bind:value={speed}>
				<option value={0.1}>0.1×</option>
				<option value={0.25}>¼×</option>
				<option value={0.5}>½×</option>
				<option value={1}>1×</option>
				<option value={2}>2×</option>
			</select>
			<label class="flex items-center gap-1" title="Restart one-shot effects when they finish"
				><input type="checkbox" class="checkbox checkbox-xs" bind:checked={loop} /> loop</label
			>
			<label class="flex items-center gap-1"
				><input type="checkbox" class="checkbox checkbox-xs" bind:checked={showGrid} /> grid</label
			>
			<select class="select select-xs w-20" bind:value={background}>
				<option value="dark">dark</option>
				<option value="grey">grey</option>
				<option value="light">light</option>
			</select>
			<button class="btn btn-ghost btn-xs" onclick={fit} title="Frame the particles"
				><Focus size={12} /> Fit</button
			>
		{/if}
	</div>
	<div class="flex min-h-0 flex-1">
		<div class="relative min-h-0 min-w-0 flex-1" bind:this={host}>
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
				approximate preview · drag: orbit · wheel: zoom
			</div>
		</div>
		{#if json}
			<div class="border-base-300 flex w-96 shrink-0 flex-col overflow-auto border-l text-xs">
				<div class="border-base-300 border-b p-2">
					{#each json.emitters as e (e.index)}
						{@const m = modelOf(e.index)}
						<div
							class="flex items-center gap-1 rounded px-1 py-0.5 {selectedEmitter ===
							e.index
								? 'bg-primary/15'
								: ''}"
						>
							<input
								type="checkbox"
								class="checkbox checkbox-xs"
								checked={visible[e.index] !== false}
								onchange={(ev) =>
									(visible = {
										...visible,
										[e.index]: (ev.currentTarget as HTMLInputElement).checked,
									})}
							/>
							<button
								class="flex-1 truncate text-left font-mono"
								onclick={() => {
									selectedEmitter = e.index;
									selectedModule = null;
								}}
								title={e.path}
							>
								{m?.name ?? e.path.split(".").pop()}
							</button>
							<span class="opacity-50"
								>{m?.meshPath ? "mesh" : "sprite"}{m && !m.enabled
									? " · off"
									: ""}</span
							>
							<span class="w-10 text-right font-mono opacity-60"
								>{counts[e.index] ?? 0}</span
							>
						</div>
						{#if m && m.unsupported.length}
							<div class="text-warning pl-6 text-[10px]">
								not simulated: {m.unsupported.map(shortClass).join(", ")}
							</div>
						{/if}
					{/each}
				</div>
				{#if emitter}
					<div class="border-base-300 border-b p-2">
						{#each moduleList as { obj, role } (obj.index)}
							<button
								class="hover:bg-base-200 block w-full truncate rounded px-1 py-0.5 text-left font-mono {selectedModule ===
								obj.index
									? 'bg-primary/15'
									: ''}"
								onclick={() =>
									(selectedModule =
										selectedModule === obj.index ? null : obj.index)}
								title={obj.path}
							>
								{shortClass(obj.class)}{role && role !== shortClass(obj.class)
									? ` (${role})`
									: ""}
							</button>
						{/each}
					</div>
				{/if}
				{#if module}
					<div class="space-y-2 p-2">
						{#if module.kind === "object"}
							{#each moduleDists as d (d.prop)}
								{#key `${module.index}:${d.prop}`}
									<CurveEditor
										dist={d}
										onchange={(nd) => patchDist(nd)}
										onapply={applyDist}
									/>
								{/key}
							{/each}
							{#if moduleDists.length === 0}
								<p class="opacity-60">
									No distribution on this module; its other properties are in the
									Properties tab.
								</p>
							{:else}
								<p class="text-[10px] opacity-50">
									Other properties (flags, enums, references) are edited in the
									Properties tab; the material's {propOf(module, "Material")
										? "texture"
										: "values"} come from the material chain.
								</p>
							{/if}
						{/if}
					</div>
				{/if}
			</div>
		{/if}
	</div>
</div>
