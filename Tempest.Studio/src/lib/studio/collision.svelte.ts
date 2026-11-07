// Collision in the map view (`map.collision`, `map.brush`, `map.setBrush`):
// what pawns and shots hit, drawn over (or instead of) the render geometry,
// and the shape of volumes. The store builds one three.js group the viewport
// adds to its scene; volumes are their convex hulls, meshes their simple
// collision (boxes, spheres, capsules, hulls) or, when they collide with
// their render triangles ("complex"), the mesh itself loaded untextured.

import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { map } from "./map.svelte";
import { rpc } from "./rpc";
import { toasts } from "./toast.svelte";
import type {
	AggGeometry,
	BrushSpec,
	CollisionFlags,
	CollisionItem,
	UpkMethods,
} from "../../../electron/src/ipc/contract";

export type CollisionData = UpkMethods["map.collision"]["result"];
export type BrushInfo = UpkMethods["map.brush"]["result"];

/** Colours by what a primitive blocks; volumes that block nothing by kind. */
export const BLOCK_COLORS: Record<CollisionFlags["blocks"], string> = {
	all: "#f38ba8",
	pawns: "#fab387",
	shots: "#89dceb",
	none: "#7f849c",
};
const VOLUME_KIND_COLORS: Record<string, string> = {
	trigger: "#a6e3a1",
	blocking: "#fab387",
	volume: "#cba6f7",
	navigation: "#94e2d5",
};
export const BLOCK_LABELS: Record<CollisionFlags["blocks"], string> = {
	all: "blocks pawns and shots",
	pawns: "blocks pawns only",
	shots: "blocks shots only",
	none: "no collision",
};

export function itemColor(item: CollisionItem): string {
	if (item.kind === "volume" && item.flags.blocks === "none") {
		return VOLUME_KIND_COLORS[item.category] ?? VOLUME_KIND_COLORS.volume;
	}
	return BLOCK_COLORS[item.flags.blocks];
}

/** Builder parameters the Shape panel edits, with UnrealEd's defaults. */
export const BRUSH_DEFAULTS: Record<
	"box" | "cylinder" | "cone" | "sphere",
	Record<string, string | number | boolean>
> = {
	box: { kind: "box", x: 256, y: 256, z: 256, hollow: false, wallThickness: 16 },
	cylinder: {
		kind: "cylinder",
		z: 256,
		outerRadius: 512,
		innerRadius: 384,
		sides: 8,
		alignToSide: true,
		hollow: false,
	},
	cone: {
		kind: "cone",
		z: 256,
		capZ: 256,
		outerRadius: 512,
		innerRadius: 384,
		sides: 8,
		alignToSide: true,
		hollow: false,
	},
	sphere: { kind: "sphere", radius: 256, sphereExtrapolation: 2 },
};

const UNREAL_TO_VIEW = 0.01;

class CollisionStore {
	/** Draw collision in the map view. */
	enabled = $state(false);
	/** Hide the render geometry while collision shows. */
	only = $state(false);
	/** Leave out what collides with nothing. */
	hideNone = $state(true);
	data = $state.raw<CollisionData | null>(null);
	loading = $state(false);
	/** Bumped when `group` is rebuilt. */
	version = $state(0);
	group: THREE.Group | null = null;
	private loadedFor: string | null = null;
	private complex = new Map<string, Promise<THREE.BufferGeometry[] | null>>();

	async toggle(on = !this.enabled): Promise<void> {
		this.enabled = on;
		if (on) await this.load();
	}

	/** Read the map's collision (again, with `force`, after an edit). */
	async load(force = false): Promise<void> {
		const path = map.path;
		if (!path) return;
		if (!force && this.data && this.loadedFor === path) return this.rebuild();
		this.loading = true;
		try {
			this.data = await rpc("map.collision", { path });
			this.loadedFor = path;
			this.rebuild();
		} catch (error) {
			toasts.error(error, "Could not read the collision");
		} finally {
			this.loading = false;
		}
	}

	/** Counts per level and per `blocks`, for the toolbar and the MCP. */
	summary(): Record<string, number> {
		const out: Record<string, number> = {};
		for (const l of this.data?.levels ?? []) {
			for (const it of l.items) {
				const k = `${it.kind}:${it.flags.blocks}`;
				out[k] = (out[k] ?? 0) + 1;
			}
		}
		return out;
	}

	private material(color: string, wire = false): THREE.Material {
		return wire
			? new THREE.LineBasicMaterial({ color, transparent: true, opacity: 0.8 })
			: new THREE.MeshBasicMaterial({
					color,
					transparent: true,
					opacity: 0.32,
					side: THREE.DoubleSide,
					depthWrite: false,
				});
	}

	private solid(geo: THREE.BufferGeometry, color: string): THREE.Object3D {
		const g = new THREE.Group();
		g.add(new THREE.Mesh(geo, this.material(color)));
		g.add(new THREE.LineSegments(new THREE.EdgesGeometry(geo, 20), this.material(color, true)));
		return g;
	}

	/** Aggregate geometry (view space already for hulls; elements in Unreal units under their TM). */
	private agg(geom: AggGeometry, color: string): THREE.Group {
		const g = new THREE.Group();
		if (geom.positions && geom.indices && geom.indices.length > 0) {
			const geo = new THREE.BufferGeometry();
			geo.setAttribute("position", new THREE.Float32BufferAttribute(geom.positions, 3));
			geo.setIndex(geom.indices);
			g.add(this.solid(geo, color));
		}
		const placed = (geo: THREE.BufferGeometry, m: number[]) => {
			const o = this.solid(geo, color);
			o.matrixAutoUpdate = false;
			o.matrix.fromArray(m);
			g.add(o);
		};
		const s = UNREAL_TO_VIEW;
		for (const b of geom.boxes ?? [])
		// View space swaps Y and Z.
		{
			placed(new THREE.BoxGeometry(b.x * s, b.z * s, b.y * s), b.matrix);
		}
		for (const sp of geom.spheres ?? []) {
			placed(new THREE.SphereGeometry(sp.radius * s, 16, 12), sp.matrix);
		}
		for (const c of geom.sphyls ?? []) {
			placed(new THREE.CapsuleGeometry(c.radius * s, c.length * s, 6, 12), c.matrix);
		}
		return g;
	}

	/** A mesh's render triangles, for complex collision. */
	private complexGeometry(mesh: {
		path?: string;
		index?: number;
		object?: string;
	}): Promise<THREE.BufferGeometry[] | null> {
		const key = `${mesh.path}#${mesh.index}#${mesh.object}`;
		let p = this.complex.get(key);
		if (!p) {
			p = (async () => {
				try {
					if (!mesh.path) return null;
					const r = await rpc("mesh.preview", {
						path: mesh.path,
						index: mesh.index as number,
						...(mesh.index === undefined ? { object: mesh.object } : {}),
						lod: 0,
						textures: false,
						skin: false,
					});
					const glb = r.glb ?? new Uint8Array();
					const gltf = await new GLTFLoader().parseAsync(
						glb.buffer.slice(
							glb.byteOffset,
							glb.byteOffset + glb.byteLength,
						) as ArrayBuffer,
						"",
					);
					const out: THREE.BufferGeometry[] = [];
					gltf.scene.updateMatrixWorld(true);
					gltf.scene.traverse((o) => {
						if (o instanceof THREE.Mesh) {
							out.push(
								(o.geometry as THREE.BufferGeometry)
									.clone()
									.applyMatrix4(o.matrixWorld),
							);
						}
					});
					return out;
				} catch {
					return null;
				}
			})();
			this.complex.set(key, p);
		}
		return p;
	}

	/** One group for the whole map: per item its world matrix, then what it collides with. */
	rebuild(): void {
		const data = this.data;
		const group = new THREE.Group();
		group.name = "collision";
		if (data) {
			for (const level of data.levels) {
				if (map.hiddenLevels[level.path]) continue;
				for (const it of level.items) {
					if (this.hideNone && it.flags.blocks === "none" && it.kind === "mesh") continue;
					const node = new THREE.Group();
					node.matrixAutoUpdate = false;
					node.matrix.fromArray(it.matrix);
					node.userData = {
						level: level.path,
						export: it.kind === "mesh" ? it.parent : it.export,
					};
					const color = itemColor(it);
					if (it.kind === "volume") {
						if (
							this.hideNone &&
							it.flags.blocks === "none" &&
							it.category !== "trigger"
						) {
							continue;
						}
						node.add(this.agg(it.geometry, color));
					} else {
						const mc = data.meshes[it.meshKey];
						if (!mc) continue;
						const how = it.flags.blocks === "shots" ? mc.shots : mc.pawns;
						if (how === "simple" && mc.simple) node.add(this.agg(mc.simple, color));
						else if (how === "complex") {
							void this.complexGeometry(mc).then((geos) => {
								for (const g of geos ?? []) node.add(this.solid(g, color));
								this.version++;
							});
						} else continue;
					}
					group.add(node);
				}
			}
		}
		this.group?.traverse((o) => {
			if (o instanceof THREE.Mesh || o instanceof THREE.LineSegments) {
				o.geometry.dispose();
				(o.material as THREE.Material).dispose();
			}
		});
		this.group = group;
		this.version++;
	}

	/** The collision items of an actor (a volume, its mesh components, or one collection mesh). */
	itemsOf(level: string, exp: number): CollisionItem[] {
		const l = this.data?.levels.find((x) => x.path === level);
		return (l?.items ?? []).filter(
			(it) => it.export === exp || (it.kind === "mesh" && it.parent === exp),
		);
	}

	/** Write a collision switch (`CollideActors`, `BlockActors`…) on an actor or component. */
	async setFlag(level: string, index: number, prop: string, value: boolean): Promise<void> {
		try {
			await rpc("object.set", { path: level, index, prop, value: value ? "True" : "False" });
			map.noteEdit(level);
			await map.reloadLevel(level);
			await this.load(true);
		} catch (error) {
			toasts.error(error, `Could not change ${prop}`);
		}
	}

	// ----- volume shapes -----------------------------------------------------

	async brush(level: string, index: number): Promise<BrushInfo | null> {
		try {
			return await rpc("map.brush", { path: level, index });
		} catch (error) {
			toasts.error(error, "Could not read the volume's shape");
			return null;
		}
	}

	/** Reshape a volume, then redraw its level (and the collision when shown). */
	async setBrush(level: string, index: number, brush: BrushSpec): Promise<boolean> {
		try {
			const r = await rpc("map.setBrush", {
				path: level,
				index,
				brush: $state.snapshot(brush) as BrushSpec,
			});
			map.noteEdit(level);
			await map.reloadLevel(level);
			if (this.enabled) await this.load(true);
			toasts.push(
				"success",
				`${r.polys} polys, ${r.hulls} convex hull${r.hulls === 1 ? "" : "s"}`,
				"Shape rebuilt",
			);
			return true;
		} catch (error) {
			toasts.error(error, "Could not rebuild the shape");
			return false;
		}
	}
}

export const collision = new CollisionStore();
