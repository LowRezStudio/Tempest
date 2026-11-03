// Where to draw the nodes of a Kismet sequence. The cooker drops the node
// positions but keeps every connector's position on the editor canvas
// (`DrawY` of inputs and outputs, `DrawX` of variable and event
// connectors), so a node's top comes from its connectors' Y and its left
// from its variable connectors' X. Nodes without one are placed next to
// what they link to; variables under the connector that reads them; the
// rest in a row below. Positions the user dragged win.
import type { KismetNode } from "../../../electron/src/ipc/contract";

export type Box = { x: number; y: number; w: number; h: number };

/** Title bar and connector spacing of a drawn node (CSS pixels at zoom 1). */
export const TITLE = 22;
export const ROW = 20;
const GAP = 80;

export function nodeSize(n: KismetNode): { w: number; h: number } {
	if (n.kind === "variable") return { w: 64, h: 64 };
	// Hidden connectors are drawn too (dimmed): links may still use them.
	const rows = Math.max(n.inputs.length, n.outputs.length, 1);
	const vars = n.variables.length + n.events.length;
	const w = Math.max(n.width ?? 0, 150, vars * 64);
	const h = TITLE + rows * ROW + (vars > 0 ? 34 : 8);
	return { w, h };
}

const minOf = (xs: (number | null)[]) => {
	const v = xs.filter((x): x is number => typeof x === "number");
	return v.length > 0 ? Math.min(...v) : null;
};

export function layout(
	nodes: KismetNode[],
	moved: Record<number, { x: number; y: number }> = {},
): Map<number, Box> {
	const out = new Map<number, Box>();
	const partial = new Map<number, { x: number | null; y: number | null; w: number; h: number }>();
	for (const n of nodes) {
		if (n.kind === "frame") continue;
		const { w, h } = nodeSize(n);
		const user = moved[n.export];
		if (user) {
			partial.set(n.export, { x: user.x, y: user.y, w, h });
			continue;
		}
		if (n.posX !== null && n.posY !== null && (n.posX !== 0 || n.posY !== 0)) {
			partial.set(n.export, { x: n.posX, y: n.posY, w, h });
			continue;
		}
		const top = minOf([...n.inputs.map((c) => c.drawY), ...n.outputs.map((c) => c.drawY)]);
		const varsX = [...n.variables.map((v) => v.drawX), ...n.events.map((v) => v.drawX)];
		const left = minOf(varsX);
		const count = Math.max(varsX.filter((x) => x !== null).length, 1);
		partial.set(n.export, {
			x: left === null ? null : left - (n.width ?? w) / (count + 1),
			y: top === null ? null : top - TITLE - ROW / 2,
			w,
			h,
		});
	}
	const byId = new Map(nodes.map((n) => [n.export, n]));
	// Links between ops: place unknown x (and y) next to a placed neighbour.
	for (let pass = 0; pass < 6; pass++) {
		let changed = false;
		for (const n of nodes) {
			const me = partial.get(n.export);
			if (!me) continue;
			for (const o of n.outputs) {
				for (const l of o.links) {
					const t = l.op === null ? undefined : partial.get(l.op);
					if (!t) continue;
					if (me.x === null && t.x !== null) {
						me.x = t.x - me.w - GAP;
						changed = true;
					} else if (t.x === null && me.x !== null) {
						t.x = me.x + me.w + GAP;
						changed = true;
					}
					if (me.y === null && t.y !== null) {
						me.y = t.y;
						changed = true;
					} else if (t.y === null && me.y !== null) {
						t.y = me.y;
						changed = true;
					}
				}
			}
		}
		if (!changed) break;
	}
	// Variables: under the first connector that names them.
	for (const n of nodes) {
		const me = partial.get(n.export);
		if (!me || n.kind === "variable") continue;
		for (const [k, v] of n.variables.entries()) {
			for (const id of v.linked) {
				const t = id === null ? undefined : partial.get(id);
				if (!t || (t.x !== null && t.y !== null) || byId.get(id!)?.kind !== "variable") {
					continue;
				}
				const vx = v.drawX ?? (me.x ?? 0) + ((k + 1) * me.w) / (n.variables.length + 1);
				t.x = vx - t.w / 2 + v.linked.indexOf(id) * (t.w + 12);
				t.y = (me.y ?? 0) + me.h + 40;
			}
		}
	}
	// Whatever is left: a row under everything placed.
	let bottom = 0;
	let right = 0;
	for (const b of partial.values()) {
		if (b.y !== null) bottom = Math.max(bottom, b.y + b.h);
		if (b.x !== null) right = Math.min(right, b.x);
	}
	let cursor = right;
	for (const [id, b] of partial) {
		const x = b.x ?? cursor;
		const y = b.y ?? bottom + 80;
		if (b.x === null || b.y === null) cursor += b.w + 40;
		out.set(id, { x, y, w: b.w, h: b.h });
	}
	return out;
}

/** Where a connector sits on its node's box. */
export function inputPoint(b: Box, k: number): { x: number; y: number } {
	return { x: b.x, y: b.y + TITLE + k * ROW + ROW / 2 };
}
export function outputPoint(b: Box, k: number): { x: number; y: number } {
	return { x: b.x + b.w, y: b.y + TITLE + k * ROW + ROW / 2 };
}
export function variablePoint(b: Box, k: number, count: number): { x: number; y: number } {
	return { x: b.x + ((k + 1) * b.w) / (count + 1), y: b.y + b.h };
}
