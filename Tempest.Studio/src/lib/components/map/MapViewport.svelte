<!--
	The map in three.js: every level's actors as nodes carrying their world
	matrix, what they draw as children (meshes loaded once per mesh and
	material set through `mesh.preview`, brushes, cylinders, light and actor
	markers). Orbit with the left button, pan with the right, wheel to zoom,
	WASD/QE to fly, F to frame the selection; click to select, the gizmo
	moves, rotates or scales (W/E/R) and writes the actor on release.
-->
<script lang="ts">
	import { onDestroy, untrack } from "svelte";
	import * as THREE from "three";
	import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
	import { TransformControls } from "three/examples/jsm/controls/TransformControls.js";
	import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
	import { categoryColor, map, TEAM_COLORS, type Shading } from "$lib/studio/map.svelte";
	import { rpc } from "$lib/studio/rpc";
	import type { MapActor, MapPart } from "../../../../electron/src/ipc/contract";

	let canvas = $state<HTMLCanvasElement | null>(null);
	let host = $state<HTMLDivElement | null>(null);
	let progress = $state<{ done: number; total: number; failed: number }>({
		done: 0,
		total: 0,
		failed: 0,
	});

	let renderer: THREE.WebGLRenderer | null = null;
	let scene: THREE.Scene | null = null;
	let camera: THREE.PerspectiveCamera | null = null;
	let controls: OrbitControls | null = null;
	let gizmo: TransformControls | null = null;
	let root: THREE.Group | null = null;
	let selectionBox: THREE.BoxHelper | null = null;
	let frame = 0;
	let resizeObserver: ResizeObserver | null = null;
	/** Actor nodes by `level#export`. */
	const nodes = new Map<string, THREE.Object3D>();
	/** Loaded meshes by request key (shared by every placement). */
	const meshCache = new Map<string, Promise<THREE.Object3D | null>>();
	const keysDown = new Set<string>();
	/** Which map the nodes were built for (a reload of one level keeps the camera). */
	let builtFor: string | null = null;
	let lastTime = performance.now();
	let draggingGizmo = false;

	// Render on demand: an idle map (thousands of meshes) redrawn at the
	// display's refresh rate kept a GPU busy for nothing.
	let needsRender = true;
	let activeUntil = 0;
	let lastRender = 0;
	/** Draw the next frame, and keep drawing for `ms` (damping, animations). */
	function invalidate(ms = 0) {
		needsRender = true;
		activeUntil = Math.max(activeUntil, performance.now() + ms);
	}

	const nodeKey = (level: string, exp: number) => `${level}#${exp}`;

	function setup() {
		if (!canvas || !host || renderer) return;
		renderer = new THREE.WebGLRenderer({
			canvas,
			antialias: true,
			logarithmicDepthBuffer: true,
		});
		renderer.setPixelRatio(window.devicePixelRatio);
		renderer.outputColorSpace = THREE.SRGBColorSpace;
		scene = new THREE.Scene();
		scene.background = new THREE.Color(0x181825);
		camera = new THREE.PerspectiveCamera(60, 1, 0.05, 20000);
		camera.position.set(40, 40, 40);
		controls = new OrbitControls(camera, canvas);
		controls.enableDamping = true;
		controls.screenSpacePanning = true;
		// Right button looks around (and flies with the movement keys), middle pans.
		controls.mouseButtons = { LEFT: THREE.MOUSE.ROTATE, MIDDLE: THREE.MOUSE.PAN, RIGHT: null };
		scene.add(new THREE.HemisphereLight(0xffffff, 0x45475a, 1.6));
		const sun = new THREE.DirectionalLight(0xffffff, 1.6);
		sun.position.set(30, 60, 20);
		scene.add(sun);
		root = new THREE.Group();
		scene.add(root);
		// The screenshot harness inspects the scene in dev builds.
		if (import.meta.env.DEV) {
			(window as unknown as { __mapViewport: unknown }).__mapViewport = { scene, nodes };
		}
		gizmo = new TransformControls(camera, canvas);
		gizmo.setSize(0.8);
		gizmo.addEventListener("dragging-changed", (e) => {
			const dragging = Boolean((e as unknown as { value: boolean }).value);
			draggingGizmo = dragging;
			if (controls) controls.enabled = !dragging;
			if (!dragging) commitGizmo();
		});
		gizmo.addEventListener("objectChange", syncProxy);
		gizmo.addEventListener("change", () => invalidate());
		controls.addEventListener("change", () => invalidate(100));
		scene.add(gizmo.getHelper());
		resizeObserver = new ResizeObserver(resize);
		resizeObserver.observe(host);
		resize();
		const loop = () => {
			frame = requestAnimationFrame(loop);
			const flying = keysDown.size > 0;
			fly();
			const moved = controls?.update() ?? false;
			const now = performance.now();
			// A slow refresh still runs, in case a change was not announced.
			if (
				!needsRender &&
				!moved &&
				!flying &&
				!draggingGizmo &&
				now >= activeUntil &&
				now - lastRender < 1000
			) {
				return;
			}
			needsRender = false;
			lastRender = now;
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
		invalidate();
	}

	/**
	 * Fly: camera and orbit target move together. Keys are physical (`code`),
	 * so ZQSD on an AZERTY keyboard is the same as WASD: W S forward/back,
	 * A D left/right, Q E down/up while the right button is held; the arrows
	 * and Page Up/Down always.
	 */
	function fly() {
		const now = performance.now();
		const dt = Math.min(0.1, (now - lastTime) / 1000);
		lastTime = now;
		if (!camera || !controls || keysDown.size === 0 || draggingGizmo) return;
		const speed = (keysDown.has("shift") ? 60 : 15) * dt;
		const forward = new THREE.Vector3();
		camera.getWorldDirection(forward);
		const right = new THREE.Vector3().crossVectors(forward, camera.up).normalize();
		const move = new THREE.Vector3();
		const held = (...codes: string[]) => codes.some((c) => keysDown.has(c));
		if (held("KeyW", "ArrowUp")) move.add(forward);
		if (held("KeyS", "ArrowDown")) move.sub(forward);
		if (held("KeyD", "ArrowRight")) move.add(right);
		if (held("KeyA", "ArrowLeft")) move.sub(right);
		if (held("KeyE", "PageUp")) move.y += 1;
		if (held("KeyQ", "PageDown")) move.y -= 1;
		if (move.lengthSq() === 0) return;
		move.normalize().multiplyScalar(speed);
		camera.position.add(move);
		controls.target.add(move);
	}

	// ----- building -------------------------------------------------------------

	function markerMaterial(color: string, opacity = 1) {
		return new THREE.MeshBasicMaterial({
			color,
			transparent: opacity < 1,
			opacity,
			depthWrite: opacity >= 1,
		});
	}

	/** A small shape standing for an actor that draws nothing itself. */
	function marker(actor: MapActor): THREE.Object3D {
		const g = new THREE.Group();
		const team = Number(actor.info.TeamIndex);
		const color =
			actor.category === "playerstart" && Number.isFinite(team)
				? (TEAM_COLORS[team] ?? categoryColor(actor.category))
				: categoryColor(actor.category);
		const scale = actor.drawScale && actor.drawScale > 0 ? 1 / actor.drawScale : 1;
		const body = new THREE.Mesh(new THREE.OctahedronGeometry(0.35), markerMaterial(color));
		body.userData.marker = true;
		g.add(body);
		if (["playerstart", "spawn", "botfactory", "gameplay"].includes(actor.category)) {
			// Unreal +X is where the actor faces.
			const arrow = new THREE.ArrowHelper(
				new THREE.Vector3(1, 0, 0),
				new THREE.Vector3(),
				1.2,
				new THREE.Color(color),
				0.4,
				0.25,
			);
			g.add(arrow);
		}
		g.scale.setScalar(scale);
		return g;
	}

	function brushMesh(part: Extract<MapPart, { kind: "brush" }>, color: string): THREE.Object3D {
		const geo = new THREE.BufferGeometry();
		geo.setAttribute("position", new THREE.Float32BufferAttribute(part.positions, 3));
		geo.setIndex(part.indices);
		geo.computeVertexNormals();
		const g = new THREE.Group();
		const fill = new THREE.Mesh(
			geo,
			new THREE.MeshBasicMaterial({
				color,
				transparent: true,
				opacity: 0.04,
				depthWrite: false,
				side: THREE.DoubleSide,
			}),
		);
		fill.userData.helper = true;
		fill.userData.brushFill = true;
		g.add(fill);
		const edges = new THREE.LineSegments(
			new THREE.EdgesGeometry(geo, 20),
			new THREE.LineBasicMaterial({ color }),
		);
		edges.userData.helper = true;
		g.add(edges);
		return g;
	}

	function matrixOf(a: number[]): THREE.Matrix4 {
		return new THREE.Matrix4().fromArray(a);
	}

	function meshKey(part: Extract<MapPart, { kind: "mesh" }>): string {
		const m = part.mesh;
		return `${m.path ?? ""}|${m.index ?? m.object ?? ""}|${part.materials.join(",")}`;
	}

	/** Shade a loaded mesh: keep its materials and make the variants on demand. */
	function prepare(model: THREE.Object3D) {
		model.traverse((o) => {
			if (!(o instanceof THREE.Mesh)) return;
			const mats = Array.isArray(o.material) ? o.material : [o.material];
			for (const m of mats) {
				if (m instanceof THREE.MeshStandardMaterial) {
					m.side = THREE.DoubleSide;
					m.metalness = 0;
					m.roughness = 0.85;
					if (!m.map) m.color.set(0x9399b2);
				}
			}
			// A flag, not the materials: `clone()` copies userData through JSON.
			o.userData.shade = true;
		});
	}

	/** The lit materials of a mesh (a clone starts with its source's, shared). */
	const litMaterials = new WeakMap<THREE.Mesh, THREE.Material | THREE.Material[]>();

	const unlitCache = new WeakMap<THREE.Material, THREE.Material>();
	const plain = new THREE.MeshStandardMaterial({
		color: 0xb4befe,
		roughness: 0.9,
		metalness: 0,
		side: THREE.DoubleSide,
	});
	const wire = new THREE.MeshBasicMaterial({ color: 0xa6adc8, wireframe: true });

	/**
	 * Surfaces of actors with `m_bCanBaseOn` False ("rooftops": pawns slide
	 * off): by the slope of each face, walls (where it should never be,
	 * the heavy walls) red, slopes orange, floors and roofs grey.
	 */
	const rooftop = new THREE.ShaderMaterial({
		side: THREE.DoubleSide,
		uniforms: { opacity: { value: 1 } },
		// The view uses a logarithmic depth buffer: custom shaders must write it too.
		vertexShader: `
			#include <common>
			#include <logdepthbuf_pars_vertex>
			varying vec3 vNormal;
			void main() {
				vNormal = normalize(mat3(modelMatrix) * normal);
				gl_Position = projectionMatrix * viewMatrix * modelMatrix * vec4(position, 1.0);
				#include <logdepthbuf_vertex>
			}`,
		fragmentShader: `
			#include <common>
			#include <logdepthbuf_pars_fragment>
			uniform float opacity;
			varying vec3 vNormal;
			void main() {
				#include <logdepthbuf_fragment>
				vec3 n = normalize(vNormal);
				float up = abs(n.y);
				vec3 wall = vec3(0.95, 0.30, 0.40);
				vec3 slope = vec3(0.98, 0.60, 0.30);
				vec3 roof = vec3(0.58, 0.60, 0.70);
				vec3 c = up < 0.35 ? wall : (up < 0.75 ? slope : roof);
				float light = 0.55 + 0.45 * abs(dot(n, normalize(vec3(0.4, 0.8, 0.3))));
				gl_FragColor = vec4(c * light, opacity);
				#include <colorspace_fragment>
			}`,
	});

	/** Volumes (often huge) stay see-through. */
	const rooftopBrush = rooftop.clone();
	rooftopBrush.uniforms = { opacity: { value: 0.35 } };
	rooftopBrush.transparent = true;
	rooftopBrush.depthWrite = false;

	/** "X-ray": the whole flagged object one flat colour, seen through everything. */
	const xray = new THREE.MeshBasicMaterial({
		color: 0xff2d55,
		transparent: true,
		opacity: 0.6,
		depthTest: false,
		depthWrite: false,
		side: THREE.DoubleSide,
	});
	const xrayBrush = new THREE.MeshBasicMaterial({
		color: 0xffb020,
		transparent: true,
		opacity: 0.15,
		depthTest: false,
		depthWrite: false,
		side: THREE.DoubleSide,
	});

	type Mode = Shading | "rooftop" | "xray";

	/** How a node's meshes are drawn: the view's shading, or the heavy-wall colours. */
	function modeOf(o: THREE.Object3D): Mode {
		let n: THREE.Object3D | null = o;
		while (n && n.userData.export === undefined) n = n.parent;
		if (!map.heavyWalls || !n?.userData.rooftop) return map.shading;
		return map.heavyStyle === "xray" ? "xray" : "rooftop";
	}

	function shadeMaterial(m: THREE.Material, shading: Mode): THREE.Material {
		if (shading === "rooftop") return rooftop;
		if (shading === "xray") return xray;
		if (shading === "untextured") return plain;
		if (shading === "wireframe") return wire;
		if (shading === "unlit" && m instanceof THREE.MeshStandardMaterial) {
			let u = unlitCache.get(m);
			if (!u) {
				u = new THREE.MeshBasicMaterial({
					map: m.map,
					color: m.map ? 0xffffff : m.color,
					side: THREE.DoubleSide,
					transparent: m.transparent,
					alphaTest: m.alphaTest,
				});
				unlitCache.set(m, u);
			}
			return u;
		}
		return m;
	}

	const brushMaterials = new WeakMap<THREE.Mesh, THREE.Material>();

	/** Shade everything under `object` by each node's mode. */
	function applyShading(object: THREE.Object3D) {
		object.traverse((o) => {
			if (o instanceof THREE.Mesh && o.userData.brushFill) {
				let own = brushMaterials.get(o);
				if (!own) {
					own = o.material as THREE.Material;
					brushMaterials.set(o, own);
				}
				const mode = modeOf(o);
				o.material = mode === "rooftop" ? rooftopBrush : mode === "xray" ? xrayBrush : own;
				o.renderOrder = mode === "xray" ? 10 : 0;
				return;
			}
			if (!(o instanceof THREE.Mesh) || !o.userData.shade) return;
			const shading = modeOf(o);
			// X-ray draws after everything else so nothing hides them.
			o.renderOrder = shading === "xray" ? 11 : 0;
			let lit = litMaterials.get(o);
			if (!lit) {
				lit = o.material as THREE.Material | THREE.Material[];
				litMaterials.set(o, lit);
			}
			o.material = Array.isArray(lit)
				? lit.map((m) => shadeMaterial(m, shading))
				: shadeMaterial(lit, shading);
		});
	}

	function loadMesh(part: Extract<MapPart, { kind: "mesh" }>): Promise<THREE.Object3D | null> {
		const key = meshKey(part);
		let p = meshCache.get(key);
		if (!p) {
			progress = { ...progress, total: progress.total + 1 };
			p = queue(async () => {
				try {
					if (!part.mesh.path) throw new Error(`${part.mesh.object} is in no package`);
					const r = await rpc("mesh.preview", {
						path: part.mesh.path,
						index: part.mesh.index as number,
						...(part.mesh.index === undefined ? { object: part.mesh.object } : {}),
						lod: 0,
						textures: true,
						skin: false,
						textureSize: 512,
						materials: part.materials.length > 0 ? part.materials : undefined,
					});
					const glb = r.glb ?? new Uint8Array();
					const gltf = await new GLTFLoader().parseAsync(
						glb.buffer.slice(
							glb.byteOffset,
							glb.byteOffset + glb.byteLength,
						) as ArrayBuffer,
						"",
					);
					prepare(gltf.scene);
					progress = { ...progress, done: progress.done + 1 };
					return gltf.scene;
				} catch {
					progress = {
						...progress,
						done: progress.done + 1,
						failed: progress.failed + 1,
					};
					return null;
				}
			});
			meshCache.set(key, p);
		}
		return p;
	}

	// Two mesh requests in flight: the backend answers one at a time anyway.
	let running = 0;
	const waiting: (() => void)[] = [];
	async function queue<T>(job: () => Promise<T>): Promise<T> {
		if (running >= 2) await new Promise<void>((r) => waiting.push(r));
		running++;
		try {
			return await job();
		} finally {
			running--;
			waiting.shift()?.();
		}
	}

	function buildActor(level: string, actor: MapActor): THREE.Object3D {
		const node = new THREE.Group();
		node.matrixAutoUpdate = false;
		node.matrix.copy(matrixOf(actor.matrix));
		node.userData = {
			level,
			export: actor.export,
			category: actor.category,
			rooftop: actor.canBaseOn === false,
		};
		const color = categoryColor(actor.category);
		let drawsSomething = false;
		for (const part of actor.parts) {
			if (part.kind === "mesh") {
				if (part.hidden) continue;
				drawsSomething = true;
				const holder = new THREE.Group();
				holder.matrixAutoUpdate = false;
				holder.matrix.copy(matrixOf(part.matrix));
				node.add(holder);
				void loadMesh(part).then((model) => {
					if (!model) {
						const box = new THREE.Mesh(
							new THREE.BoxGeometry(0.5, 0.5, 0.5),
							markerMaterial("#f38ba8", 0.6),
						);
						box.userData.marker = true;
						holder.add(box);
						invalidate();
						return;
					}
					const copy = model.clone();
					holder.add(copy);
					applyShading(copy);
					invalidate();
				});
			} else if (part.kind === "brush") {
				drawsSomething = true;
				node.add(brushMesh(part, color));
			} else if (part.kind === "cylinder") {
				const c = new THREE.Mesh(
					new THREE.CylinderGeometry(
						part.radius / 100,
						part.radius / 100,
						(part.height * 2) / 100,
						16,
						1,
						true,
					),
					new THREE.MeshBasicMaterial({
						color,
						wireframe: true,
						transparent: true,
						opacity: 0.5,
					}),
				);
				c.userData.helper = true;
				node.add(c);
			} else if (part.kind === "light") {
				drawsSomething = true;
				const l = new THREE.Group();
				l.matrixAutoUpdate = false;
				l.matrix.copy(matrixOf(part.matrix));
				const [r, g, b] = part.color;
				const bulb = new THREE.Mesh(
					new THREE.SphereGeometry(0.2, 8, 6),
					markerMaterial(`rgb(${r},${g},${b})`),
				);
				bulb.userData.marker = true;
				l.add(bulb);
				const reach = new THREE.Mesh(
					new THREE.SphereGeometry(part.radius / 100, 16, 8),
					new THREE.MeshBasicMaterial({
						color: `rgb(${r},${g},${b})`,
						wireframe: true,
						transparent: true,
						opacity: 0.08,
					}),
				);
				reach.userData.helper = true;
				reach.userData.lightRadius = true;
				reach.visible = false;
				l.add(reach);
				node.add(l);
			} else if (part.kind === "particles") {
				const p = new THREE.Mesh(new THREE.TetrahedronGeometry(0.3), markerMaterial(color));
				p.userData.marker = true;
				node.add(p);
				drawsSomething = true;
			}
		}
		if (!drawsSomething) node.add(marker(actor));
		return node;
	}

	function build() {
		if (!root) return;
		for (const child of [...root.children]) root.remove(child);
		nodes.clear();
		gizmo?.detach();
		for (const level of map.levels) {
			const group = new THREE.Group();
			group.userData.levelGroup = level.path;
			for (const actor of level.actors) {
				const node = buildActor(level.path, actor);
				nodes.set(nodeKey(level.path, actor.export), node);
				group.add(node);
			}
			root.add(group);
		}
		applyVisibility();
	}

	function applyVisibility() {
		if (!root) return;
		for (const group of root.children) {
			const level = group.userData.levelGroup as string;
			group.visible = !map.hiddenLevels[level];
			for (const n of group.children) {
				// The heavy-wall view shows rooftop-flagged volumes even when volumes are hidden.
				n.visible =
					!map.hiddenCategories[n.userData.category as string] ||
					(map.heavyWalls && Boolean(n.userData.rooftop));
			}
		}
		const sel = map.selected && nodes.get(nodeKey(map.selected.level, map.selected.export));
		if (sel) sel.visible = true;
	}

	/** Frame the playable area: player starts and gameplay actors (skyboxes are far bigger), else everything. */
	function frameAll() {
		if (!root || !camera || !controls) return;
		const box = new THREE.Box3();
		const play = new Set(["playerstart", "spawn", "botfactory", "gameplay"]);
		for (const only of [true, false]) {
			for (const group of root.children) {
				for (const n of group.children) {
					const c = n.userData.category as string;
					if (c === "info" || (only && !play.has(c))) continue;
					box.expandByPoint(new THREE.Vector3().setFromMatrixPosition(n.matrix));
				}
			}
			if (!box.isEmpty()) break;
		}
		if (box.isEmpty()) return;
		const center = box.getCenter(new THREE.Vector3());
		const size = box.getSize(new THREE.Vector3());
		const radius = Math.max(size.x, size.z, 10) * 0.6;
		controls.target.copy(center);
		camera.position
			.copy(center)
			.add(new THREE.Vector3(radius * 0.6, radius * 0.8, radius * 0.6));
		controls.update();
	}

	function frameSelection() {
		const n = map.selected && nodes.get(nodeKey(map.selected.level, map.selected.export));
		if (!n || !camera || !controls) return;
		n.updateMatrixWorld(true);
		const box = new THREE.Box3().setFromObject(n);
		const center = box.isEmpty()
			? new THREE.Vector3().setFromMatrixPosition(n.matrixWorld)
			: box.getCenter(new THREE.Vector3());
		const radius = box.isEmpty() ? 6 : Math.max(box.getSize(new THREE.Vector3()).length(), 6);
		const dir = camera.position.clone().sub(controls.target).normalize();
		controls.target.copy(center);
		camera.position.copy(center).add(dir.multiplyScalar(radius * 1.5));
		controls.update();
	}

	// ----- selection and gizmo ----------------------------------------------------

	/** A proxy the gizmo moves (nodes keep their matrices, gizmos want position/quaternion/scale). */
	let proxy: THREE.Object3D | null = null;

	function showSelection() {
		if (!scene || !gizmo) return;
		if (selectionBox) {
			scene.remove(selectionBox);
			selectionBox.dispose();
			selectionBox = null;
		}
		gizmo.detach();
		const sel = map.selected;
		const n = sel && nodes.get(nodeKey(sel.level, sel.export));
		for (const node of nodes.values()) {
			node.traverse((o) => {
				if (o.userData.lightRadius) o.visible = node === n;
			});
		}
		if (!n) return;
		n.updateMatrixWorld(true);
		selectionBox = new THREE.BoxHelper(n, 0xf9e2af);
		scene.add(selectionBox);
		if (map.tool === "select") return;
		if (!proxy) {
			proxy = new THREE.Object3D();
			scene.add(proxy);
		}
		n.matrix.decompose(proxy.position, proxy.quaternion, proxy.scale);
		proxy.updateMatrixWorld(true);
		proxy.userData.node = n;
		gizmo.attach(proxy);
		gizmo.setMode(map.tool);
		gizmo.setSpace(map.tool === "scale" ? "local" : "world");
	}

	/** While dragging, the node follows the proxy; on release the actor is written. */
	function syncProxy() {
		if (!proxy || !draggingGizmo) return;
		const n = proxy.userData.node as THREE.Object3D | undefined;
		if (!n) return;
		proxy.updateMatrix();
		n.matrix.copy(proxy.matrix);
		n.updateMatrixWorld(true);
		selectionBox?.update();
	}

	function commitGizmo() {
		const sel = map.selected;
		const n = proxy?.userData.node as THREE.Object3D | undefined;
		if (!sel || !n || !proxy) return;
		proxy.updateMatrix();
		n.matrix.copy(proxy.matrix);
		const actor = map.actor(sel);
		if (!actor) return;
		const m = n.matrix.toArray();
		if (m.every((v, i) => Math.abs(v - actor.matrix[i]) < 1e-6)) return;
		void map.move(sel.level, actor, m);
	}

	let down: { x: number; y: number } | null = null;
	function onPointerDown(e: PointerEvent) {
		down = { x: e.clientX, y: e.clientY };
	}

	function onPointerUp(e: PointerEvent) {
		if (!down || !canvas || !camera || !root || draggingGizmo) return;
		const moved = Math.hypot(e.clientX - down.x, e.clientY - down.y);
		down = null;
		if (moved > 4 || e.button !== 0) return;
		const rect = canvas.getBoundingClientRect();
		const pointer = new THREE.Vector2(
			((e.clientX - rect.left) / rect.width) * 2 - 1,
			-((e.clientY - rect.top) / rect.height) * 2 + 1,
		);
		const ray = new THREE.Raycaster();
		ray.params.Line = { threshold: 0.1 };
		ray.setFromCamera(pointer, camera);
		const hits = ray.intersectObjects(root.children, true).filter((h) => {
			let o: THREE.Object3D | null = h.object;
			while (o) {
				if (!o.visible) return false;
				o = o.parent;
			}
			// Volumes are picked by their edges only, so meshes inside stay clickable.
			return !(h.object.userData.helper && h.object instanceof THREE.Mesh);
		});
		for (const h of hits) {
			let o: THREE.Object3D | null = h.object;
			while (o && o.userData.export === undefined) o = o.parent;
			if (o) {
				void map.select({
					level: o.userData.level as string,
					export: o.userData.export as number,
				});
				return;
			}
		}
		void map.select(null);
	}

	function onKeyDown(e: KeyboardEvent) {
		const t = e.target as HTMLElement | null;
		if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
		if (e.ctrlKey || e.metaKey || e.altKey) return;
		const k = e.key.toLowerCase();
		if (e.shiftKey) keysDown.add("shift");
		const always = ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", "PageDown"];
		const whileLooking = ["KeyW", "KeyA", "KeyS", "KeyD", "KeyQ", "KeyE"];
		if (always.includes(e.code) || (looking && whileLooking.includes(e.code))) {
			keysDown.add(e.code);
			e.preventDefault();
			return;
		}
		if (k === "w") map.tool = "translate";
		else if (k === "e") map.tool = "rotate";
		else if (k === "r") map.tool = "scale";
		else if (k === "q") map.tool = "select";
		else if (k === "f") frameSelection();
		else if (k === "escape") void map.select(null);
		else return;
		e.preventDefault();
	}

	function onKeyUp(e: KeyboardEvent) {
		keysDown.delete(e.code);
		if (!e.shiftKey) keysDown.delete("shift");
	}

	/** Right button held: mouse looks around, movement keys fly. */
	let looking = false;
	let lookFrom: { x: number; y: number } | null = null;

	function startLook(e: PointerEvent) {
		if (e.button !== 2 || !canvas) return;
		looking = true;
		lookFrom = { x: e.clientX, y: e.clientY };
		canvas.setPointerCapture(e.pointerId);
	}

	function look(e: PointerEvent) {
		if (!looking || !lookFrom || !camera || !controls) return;
		const dx = e.clientX - lookFrom.x;
		const dy = e.clientY - lookFrom.y;
		lookFrom = { x: e.clientX, y: e.clientY };
		// Turn the view around the camera: the orbit target moves, the camera stays.
		const offset = controls.target.clone().sub(camera.position);
		const dist = Math.max(offset.length(), 1);
		const sph = new THREE.Spherical().setFromVector3(offset);
		sph.theta -= dx * 0.004;
		sph.phi = Math.min(Math.PI - 0.05, Math.max(0.05, sph.phi + dy * 0.004));
		sph.radius = dist;
		controls.target.copy(camera.position).add(new THREE.Vector3().setFromSpherical(sph));
		camera.lookAt(controls.target);
	}

	function stopLook() {
		looking = false;
		lookFrom = null;
		for (const c of ["KeyW", "KeyA", "KeyS", "KeyD", "KeyQ", "KeyE"]) keysDown.delete(c);
	}

	// ----- reactions --------------------------------------------------------------

	$effect(() => {
		// Rebuild when the map or its levels change.
		const levels = map.levels;
		const version = map.version;
		void version;
		untrack(() => {
			setup();
			if (levels.length === 0) return;
			const fresh = builtFor !== map.path;
			build();
			if (fresh) {
				builtFor = map.path;
				frameAll();
			}
			showSelection();
			invalidate();
		});
	});
	$effect(() => {
		void [map.hiddenCategories, map.hiddenLevels];
		for (const v of Object.values(map.hiddenCategories)) void v;
		for (const v of Object.values(map.hiddenLevels)) void v;
		untrack(() => {
			applyVisibility();
			invalidate();
		});
	});
	$effect(() => {
		void map.selected;
		void map.tool;
		untrack(() => {
			showSelection();
			invalidate();
		});
	});
	$effect(() => {
		void map.shading;
		void map.heavyWalls;
		void map.heavyStyle;
		untrack(() => {
			if (root) applyShading(root);
			applyVisibility();
			invalidate();
		});
	});
	$effect(() => {
		if (map.focusRequest > 0) {
			untrack(() => {
				frameSelection();
				invalidate(300);
			});
		}
	});

	// Matinee preview: driven actors follow the timeline, movements draw their paths.
	let posed = new Set<string>();
	let matineeLines: THREE.Group | null = null;
	$effect(() => {
		const preview = map.matineePreview;
		void map.version;
		untrack(() => {
			if (!scene) return;
			for (const k of posed) {
				if (preview.poses.has(k)) continue;
				const n = nodes.get(k);
				const [level, exp] = [
					k.slice(0, k.lastIndexOf("#")),
					Number(k.slice(k.lastIndexOf("#") + 1)),
				];
				const a = map.actor({ level, export: exp });
				if (n && a) n.matrix.fromArray(a.matrix);
			}
			for (const [k, m] of preview.poses) nodes.get(k)?.matrix.fromArray(m);
			posed = new Set(preview.poses.keys());
			if (matineeLines) {
				scene.remove(matineeLines);
				matineeLines.traverse((o) => {
					if (o instanceof THREE.Line) o.geometry.dispose();
				});
				matineeLines = null;
			}
			if (preview.paths.length > 0) {
				matineeLines = new THREE.Group();
				for (const p of preview.paths) {
					const geo = new THREE.BufferGeometry().setFromPoints(
						p.points.map(([x, y, z]) => new THREE.Vector3(x, y, z)),
					);
					matineeLines.add(
						new THREE.Line(geo, new THREE.LineBasicMaterial({ color: p.color })),
					);
					const end = p.points.at(-1)!;
					const tip = new THREE.Mesh(
						new THREE.SphereGeometry(0.15, 8, 6),
						markerMaterial(p.color),
					);
					tip.position.set(end[0], end[1], end[2]);
					matineeLines.add(tip);
				}
				scene.add(matineeLines);
			}
			selectionBox?.update();
			invalidate();
		});
	});

	onDestroy(() => {
		cancelAnimationFrame(frame);
		resizeObserver?.disconnect();
		gizmo?.dispose();
		controls?.dispose();
		renderer?.dispose();
	});
</script>

<svelte:window
	onkeydown={onKeyDown}
	onkeyup={onKeyUp}
	onblur={() => {
		keysDown.clear();
		stopLook();
	}}
/>

<div class="relative min-h-0 flex-1" bind:this={host}>
	<canvas
		bind:this={canvas}
		class="absolute inset-0 h-full w-full outline-none"
		tabindex="-1"
		onpointerdown={(e) => {
			startLook(e);
			onPointerDown(e);
			invalidate();
		}}
		onpointermove={(e) => {
			look(e);
			invalidate();
		}}
		onpointerup={(e) => {
			if (e.button === 2) stopLook();
			onPointerUp(e);
			invalidate(300);
		}}
		onwheel={() => invalidate(300)}
		oncontextmenu={(e) => e.preventDefault()}
	></canvas>
	{#if progress.total > 0 && progress.done < progress.total}
		<div class="bg-base-200/80 absolute bottom-2 left-2 rounded px-2 py-1 font-mono text-xs">
			meshes {progress.done}/{progress.total}{progress.failed
				? `, ${progress.failed} failed`
				: ""}
		</div>
	{:else if progress.failed > 0}
		<div
			class="bg-base-200/80 absolute bottom-2 left-2 rounded px-2 py-1 font-mono text-xs opacity-70"
			title="Shown as red boxes"
		>
			{progress.failed} meshes could not be loaded
		</div>
	{/if}
	{#if map.heavyWalls}
		<div class="bg-base-200/85 absolute top-2 left-2 space-y-0.5 rounded px-2 py-1.5 text-xs">
			<div class="font-semibold">Can't be stood on (m_bCanBaseOn False)</div>
			{#if map.heavyStyle === "xray"}
				<div class="flex items-center gap-1.5">
					<span class="h-2.5 w-2.5 rounded-sm" style="background:#ff2d55"></span> flagged meshes
					(seen through walls)
				</div>
				<div class="flex items-center gap-1.5">
					<span class="h-2.5 w-2.5 rounded-sm" style="background:#ffb020"></span> flagged volumes
				</div>
			{:else}
				<div class="flex items-center gap-1.5">
					<span class="h-2.5 w-2.5 rounded-sm" style="background:#f24d66"></span> walls: heavy
					walls
				</div>
				<div class="flex items-center gap-1.5">
					<span class="h-2.5 w-2.5 rounded-sm" style="background:#fa994d"></span> slopes
				</div>
				<div class="flex items-center gap-1.5">
					<span class="h-2.5 w-2.5 rounded-sm" style="background:#9499b3"></span> roofs and
					floors (as intended)
				</div>
			{/if}
			<div class="opacity-60">{map.rooftopCount} actors flagged</div>
		</div>
	{/if}
	<div
		class="pointer-events-none absolute right-2 bottom-2 text-right font-mono text-[10px] leading-tight opacity-50"
	>
		left drag orbit · right drag look, + WASD/ZQSD fly (Q E down/up) · middle drag pan · arrows
		fly · wheel zoom<br />
		click select · W move · E rotate · R scale · Q select only · F frame
	</div>
</div>
