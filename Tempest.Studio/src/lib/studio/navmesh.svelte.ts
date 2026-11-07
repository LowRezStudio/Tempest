// The Havok nav mesh of the open map (`map.navmesh`): drawn over the scene
// by `MapViewport` (faces coloured by region or cluster, edges, boundaries,
// jump links as arcs), faces picked in the view, and edits (delete faces,
// cut a region, move a vertex) written back by `map.navmeshEdit` into the
// HavokNavMeshActor of the persistent level: an unsaved, undoable edit of
// that package like any other map edit.
import { map } from "./map.svelte";
import { rpc } from "./rpc";
import { toasts } from "./toast.svelte";
import type { NavMeshActor, UpkMethods } from "../../../electron/src/ipc/contract";

export type NavColour = "region" | "cluster";

/** Decoded buffers of one nav mesh actor. */
export type NavMeshData = {
	info: Omit<
		NavMeshActor,
		| "positions"
		| "triangles"
		| "triFace"
		| "faceCluster"
		| "faceRegion"
		| "lines"
		| "lineKind"
		| "links"
	>;
	positions: Float32Array;
	triangles: Uint32Array;
	triFace: Uint32Array;
	faceCluster: Int32Array;
	faceRegion: Int32Array;
	lines: Uint32Array;
	lineKind: Uint8Array;
	links: Float32Array;
};

// The payloads are slices of one framed reply: copy them so typed views are aligned.
const f32 = (b: Uint8Array) => new Float32Array(new Uint8Array(b).buffer);
const u32 = (b: Uint8Array) => new Uint32Array(new Uint8Array(b).buffer);
const i32 = (b: Uint8Array) => new Int32Array(new Uint8Array(b).buffer);

function decode(a: NavMeshActor): NavMeshData {
	const {
		positions,
		triangles,
		triFace,
		faceCluster,
		faceRegion,
		lines,
		lineKind,
		links,
		...info
	} = a;
	return {
		info,
		positions: f32(positions),
		triangles: u32(triangles),
		triFace: u32(triFace),
		faceCluster: i32(faceCluster),
		faceRegion: i32(faceRegion),
		lines: u32(lines),
		lineKind: new Uint8Array(lineKind),
		links: f32(links),
	};
}

class NavMeshStore {
	/** Draw the nav mesh over the map. */
	show = $state(false);
	colour = $state<NavColour>("region");
	/** What clicks in the view pick: actors (off), nav mesh faces, or a vertex of the face clicked. */
	pick = $state<"off" | "faces" | "vertex">("off");
	/** The picked vertex (`pick: "vertex"`). */
	vertex = $state<number | null>(null);
	/** Draw the faces through the level's geometry. */
	xray = $state(false);
	loading = $state(false);
	error = $state<string | null>(null);
	/** The level package the data came from (the persistent map). */
	level = $state<string | null>(null);
	actors = $state.raw<NavMeshData[]>([]);
	/** Which actor (index into `actors`) edits apply to. */
	current = $state(0);
	/** Picked faces of the current actor. */
	selected = $state.raw<Set<number>>(new Set());
	/** Bumped when the buffers change (the viewport rebuilds its objects). */
	version = $state(0);
	private loadedFor: string | null = null;
	private loadedVersion = -1;

	get data(): NavMeshData | null {
		return this.actors[this.current] ?? null;
	}

	/** Read the nav mesh of the open map (again when the map or one of its levels changed). */
	async load(force = false): Promise<void> {
		const path = map.path;
		if (!path) {
			this.actors = [];
			return;
		}
		if (!force && this.loadedFor === path && this.loadedVersion === map.version) return;
		this.loading = true;
		this.error = null;
		try {
			const r = await rpc("map.navmesh", { path });
			if (map.path !== path) return;
			const keep = this.loadedFor === path;
			this.actors = r.actors.map(decode);
			this.level = path;
			this.loadedFor = path;
			this.loadedVersion = map.version;
			if (!keep) {
				this.current = 0;
				this.selected = new Set();
			} else if (this.current >= this.actors.length) this.current = 0;
			this.version++;
			if (this.actors.length === 0) this.error = "This map has no Havok nav mesh.";
		} catch (error) {
			this.error = String(error);
		} finally {
			this.loading = false;
		}
	}

	toggleFace(face: number, add: boolean): void {
		const s = new Set(add ? this.selected : []);
		if (add && s.has(face)) s.delete(face);
		else s.add(face);
		this.selected = s;
		this.version++;
	}

	clearSelection(): void {
		this.selected = new Set();
		this.vertex = null;
		this.version++;
	}

	/** Every face of the regions holding a picked face. */
	selectRegions(): void {
		const d = this.data;
		if (!d) return;
		const regions = new Set([...this.selected].map((f) => d.faceRegion[f]));
		const s = new Set<number>();
		for (let f = 0; f < d.faceRegion.length; f++) if (regions.has(d.faceRegion[f])) s.add(f);
		this.selected = s;
		this.version++;
	}

	async edit(
		op: UpkMethods["map.navmeshEdit"]["params"]["op"],
		args: { faces?: number[]; moves?: [number, number, number, number][] },
	): Promise<boolean> {
		const d = this.data;
		const level = this.level;
		if (!d || !level) return false;
		try {
			const r = await rpc("map.navmeshEdit", {
				path: level,
				index: d.info.index,
				op,
				...args,
			});
			const next = [...this.actors];
			next[this.current] = decode(r);
			this.actors = next;
			this.selected = new Set();
			this.vertex = null;
			this.version++;
			map.noteEdit(level);
			toasts.push("success", `${r.faces} faces, ${r.regions} regions`, "Nav mesh edited");
			return true;
		} catch (error) {
			toasts.error(error, "Nav mesh edit failed");
			return false;
		}
	}

	deleteSelected(): Promise<boolean> {
		return this.edit("deleteFaces", { faces: [...this.selected] });
	}

	cutRegions(): Promise<boolean> {
		return this.edit("deleteRegion", { faces: [...this.selected] });
	}

	moveVertex(vertex: number, to: [number, number, number]): Promise<boolean> {
		return this.edit("moveVertices", { moves: [[vertex, ...to]] });
	}
}

export const navmesh = new NavMeshStore();
