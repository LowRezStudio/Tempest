// three.js objects for a nav mesh (`navmesh.svelte.ts`): faces coloured
// by region or cluster, the picked faces, every edge (boundaries and user
// edges highlighted) and the jump links as arcs between their two edges.
import * as THREE from "three";
import type { NavColour, NavMeshData } from "./navmesh.svelte";

/** Lift over the level's surfaces, in metres (the faces lie on them). */
const LIFT = 0.04;

function idColour(id: number, out: THREE.Color): THREE.Color {
	// Golden-ratio hues: neighbouring ids get distant colours.
	const h = (((id * 0.618033988749895) % 1) + 1) % 1;
	return out.setHSL(h, 0.55, 0.55);
}

const LINE_COLOURS = [0x6c7086, 0xf38ba8, 0x89dceb];
const LINK_COLOUR = 0x94e2d5;

export type NavObjects = {
	group: THREE.Group;
	/** The faces' mesh; a hit's `faceIndex` is a triangle, `triFace` gives the face. */
	faces: THREE.Mesh;
};

export function buildNavMesh(
	d: NavMeshData,
	colour: NavColour,
	selected: Set<number>,
	xray: boolean,
): NavObjects {
	const group = new THREE.Group();
	group.name = "navmesh";
	const p = d.positions;
	const corner = (v: number, out: Float32Array, at: number) => {
		out[at] = p[v * 3];
		out[at + 1] = p[v * 3 + 1] + LIFT;
		out[at + 2] = p[v * 3 + 2];
	};
	const tris = d.triangles.length / 3;
	const pos = new Float32Array(tris * 9);
	const col = new Float32Array(tris * 9);
	const c = new THREE.Color();
	const sel = new THREE.Color(0xf9e2af);
	for (let t = 0; t < tris; t++) {
		const f = d.triFace[t];
		const id = colour === "cluster" ? d.faceCluster[f] : d.faceRegion[f];
		const picked = selected.has(f);
		if (picked) c.copy(sel);
		else idColour(id, c);
		for (let k = 0; k < 3; k++) {
			corner(d.triangles[t * 3 + k], pos, t * 9 + k * 3);
			col.set([c.r, c.g, c.b], t * 9 + k * 3);
		}
	}
	const geo = new THREE.BufferGeometry();
	geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
	geo.setAttribute("color", new THREE.BufferAttribute(col, 3));
	geo.computeBoundingSphere();
	const faces = new THREE.Mesh(
		geo,
		new THREE.MeshBasicMaterial({
			vertexColors: true,
			transparent: true,
			opacity: 0.45,
			side: THREE.DoubleSide,
			depthWrite: false,
			depthTest: !xray,
			polygonOffset: true,
			polygonOffsetFactor: -2,
			polygonOffsetUnits: -2,
		}),
	);
	faces.renderOrder = 10;
	group.add(faces);

	// Edges, one segment each, coloured by kind.
	const n = d.lineKind.length;
	const lp = new Float32Array(n * 6);
	const lc = new Float32Array(n * 6);
	for (let i = 0; i < n; i++) {
		corner(d.lines[i * 2], lp, i * 6);
		corner(d.lines[i * 2 + 1], lp, i * 6 + 3);
		c.setHex(LINE_COLOURS[d.lineKind[i]] ?? LINE_COLOURS[0]);
		lc.set([c.r, c.g, c.b, c.r, c.g, c.b], i * 6);
	}
	const lgeo = new THREE.BufferGeometry();
	lgeo.setAttribute("position", new THREE.BufferAttribute(lp, 3));
	lgeo.setAttribute("color", new THREE.BufferAttribute(lc, 3));
	const lines = new THREE.LineSegments(
		lgeo,
		new THREE.LineBasicMaterial({ vertexColors: true, depthTest: !xray, transparent: true }),
	);
	lines.renderOrder = 11;
	lines.userData.helper = true;
	group.add(lines);

	// Jump links: both edges, and an arc from A's middle to B's.
	const lk = d.links;
	const arc: number[] = [];
	const v = (i: number) => new THREE.Vector3(lk[i * 3], lk[i * 3 + 1] + LIFT, lk[i * 3 + 2]);
	for (let l = 0; l < lk.length / 12; l++) {
		const [a0, a1, b0, b1] = [v(l * 4), v(l * 4 + 1), v(l * 4 + 2), v(l * 4 + 3)];
		const am = a0.clone().add(a1).multiplyScalar(0.5);
		const bm = b0.clone().add(b1).multiplyScalar(0.5);
		const top = am.clone().add(bm).multiplyScalar(0.5);
		top.y = Math.max(am.y, bm.y) + 0.3 * am.distanceTo(bm) + 0.3;
		const curve = new THREE.QuadraticBezierCurve3(am, top, bm);
		const pts = curve.getPoints(16);
		for (let k = 0; k + 1 < pts.length; k++) {
			arc.push(...pts[k].toArray(), ...pts[k + 1].toArray());
		}
		arc.push(...a0.toArray(), ...a1.toArray(), ...b0.toArray(), ...b1.toArray());
	}
	if (arc.length > 0) {
		const ageo = new THREE.BufferGeometry();
		ageo.setAttribute("position", new THREE.Float32BufferAttribute(arc, 3));
		const arcs = new THREE.LineSegments(
			ageo,
			new THREE.LineBasicMaterial({
				color: LINK_COLOUR,
				depthTest: false,
				transparent: true,
			}),
		);
		arcs.renderOrder = 12;
		arcs.userData.helper = true;
		group.add(arcs);
	}
	return { group, faces };
}

export function disposeNavMesh(o: NavObjects | null): void {
	if (!o) return;
	o.group.traverse((x) => {
		if (x instanceof THREE.Mesh || x instanceof THREE.LineSegments) {
			x.geometry.dispose();
			(x.material as THREE.Material).dispose();
		}
	});
}

/** The vertex of a face nearest to a point (view space). */
export function nearestVertex(d: NavMeshData, face: number, point: THREE.Vector3): number | null {
	let best: number | null = null;
	let dist = Infinity;
	for (let t = 0; t < d.triFace.length; t++) {
		if (d.triFace[t] !== face) continue;
		for (let k = 0; k < 3; k++) {
			const vi = d.triangles[t * 3 + k];
			const dx = d.positions[vi * 3] - point.x;
			const dy = d.positions[vi * 3 + 1] - point.y;
			const dz = d.positions[vi * 3 + 2] - point.z;
			const dd = dx * dx + dy * dy + dz * dz;
			if (dd < dist) {
				dist = dd;
				best = vi;
			}
		}
	}
	return best;
}
