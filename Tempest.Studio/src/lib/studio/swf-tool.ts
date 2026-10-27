// The movie editor for agents: what the MCP tool `studio_swf` runs in the
// renderer (electron/src/mcp.ts). It drives the same `SwfDoc` as the editor
// (selecting the export mounts it), so the window follows every step, edits
// stay pending until `apply` writes them as one undo step, and the project
// and the cooker see them like any other edit.
import { previewFor } from "./classes";
import { rpc } from "./rpc";
import { studio } from "./state.svelte";
import { compose, decompose, swfDoc, type SwfDoc, type SwfNode } from "./swf.svelte";
import type { SwfEntry, SwfMatrix } from "../../../electron/src/ipc/contract";

export type SwfToolArgs = {
	action:
		| "movies"
		| "open"
		| "status"
		| "tree"
		| "timeline"
		| "info"
		| "script"
		| "edit"
		| "remove"
		| "revert"
		| "text"
		| "layers"
		| "apply"
		| "discard";
	path?: string;
	index?: number;
	object?: string;
	query?: string;
	sprite?: number;
	frame?: number;
	node?: "movie" | "frames" | "texts" | "scripts" | "images" | "linkage" | "tags";
	character?: number;
	depth?: number;
	kind?: string;
	section?: "texts" | "scripts" | "images" | "linkage" | "tags";
	tag?: number;
	text?: string;
	limit?: number;
	offset?: number;
	x?: number;
	y?: number;
	dx?: number;
	dy?: number;
	scaleX?: number;
	scaleY?: number;
	rotation?: number;
	skew?: number;
	matrix?: Partial<SwfMatrix>;
	alpha?: number;
	name?: string;
	hide?: number[];
	show?: number[];
	solo?: number[];
	showAll?: boolean;
};

const isMovie = (cls: string) => previewFor(cls) === "swf";
const r3 = (v: number) => Math.round(v * 1000) / 1000;
const spriteLabel = (doc: SwfDoc) => (doc.sprite === 0 ? "Root" : doc.label(doc.sprite ?? 0));

/** The movie an action is about: `path`/`index`/`object`, else the selected export. */
async function movie(a: SwfToolArgs): Promise<SwfDoc> {
	if (a.path && a.path !== studio.pkg?.path) {
		studio.mode = "packages";
		await studio.openFiles([a.path]);
		if (studio.pkg?.path !== a.path) throw new Error(`could not open ${a.path}`);
	}
	const pkg = studio.pkg;
	if (!pkg) throw new Error("no package is open (give path)");
	let index = a.index ?? null;
	if (index === null && a.object) {
		const want = a.object.toLowerCase();
		const hits = pkg.exports.filter(
			(r) =>
				isMovie(r.class) &&
				(r.path.toLowerCase() === want || r.name.toLowerCase() === want),
		);
		if (hits.length !== 1) {
			throw new Error(
				hits.length === 0
					? `no movie ${a.object} in ${pkg.name} (action "movies" lists them)`
					: `${a.object} names several movies: ${hits.map((r) => `${r.i} ${r.path}`).join(", ")}`,
			);
		}
		index = hits[0].i;
	}
	index ??= studio.selectedRow && isMovie(studio.selectedRow.class) ? studio.selectedRow.i : null;
	if (index === null) throw new Error('give index or object (action "movies" lists the movies)');
	const row = pkg.exports[index];
	if (!row || !isMovie(row.class)) {
		throw new Error(`export ${index} is not a movie (${row?.class ?? "none"})`);
	}
	studio.mode = "packages";
	if (studio.selected !== index) await studio.select(index);
	const doc = swfDoc(pkg.path, index);
	await doc.settled();
	if (doc.error) throw new Error(doc.error);
	return doc;
}

/** Go to `sprite` (0: the root) / `node` / `character`, then `frame` and `depth`. */
async function goTo(doc: SwfDoc, a: SwfToolArgs): Promise<void> {
	if (a.sprite !== undefined) {
		if (a.sprite !== 0 && doc.chars.get(a.sprite)?.kind !== "sprite") {
			throw new Error(`character ${a.sprite} is not a sprite`);
		}
		doc.open(a.sprite === 0 ? "frames" : `char:${a.sprite}`);
	} else if (a.character !== undefined) {
		if (!doc.chars.has(a.character)) throw new Error(`no character ${a.character}`);
		doc.openCharacter(a.character);
	} else if (a.node) {
		doc.open(a.node as SwfNode);
	}
	await doc.settled();
	if (a.frame !== undefined) {
		const n = doc.timeline?.frames.length ?? 1;
		if (a.frame < 0 || a.frame >= n) throw new Error(`frame ${a.frame} is out of 0..${n - 1}`);
		doc.frame = a.frame;
	}
	if (a.depth !== undefined && a.action === "open") doc.depth = entryAt(doc, a.depth).depth;
}

function entryAt(doc: SwfDoc, depth: number | undefined): SwfEntry {
	if (doc.sprite === null) throw new Error("open a sprite or the root timeline first (sprite)");
	if (depth === undefined) throw new Error('give depth (action "timeline" lists them)');
	const e = doc.entries.find((x) => x.depth === depth);
	if (!e) {
		throw new Error(
			`nothing at depth ${depth} on frame ${doc.frame} of ${spriteLabel(doc)}; depths: ${doc.entries.map((x) => x.depth).join(", ")}`,
		);
	}
	return e;
}

function entryJson(doc: SwfDoc, e: SwfEntry) {
	const d = decompose(e.matrix);
	const c = doc.chars.get(e.character);
	return {
		depth: e.depth,
		character: e.character,
		label: doc.label(e.character),
		kind: c?.kind ?? null,
		name: e.name,
		className: e.className,
		x: r3(d.x),
		y: r3(d.y),
		scaleX: r3(d.sx),
		scaleY: r3(d.sy),
		rotation: r3(d.rot),
		skew: r3(d.skew),
		alpha: r3((e.cxform?.mul[3] ?? 1) * 100),
		cxform: e.cxform,
		rect: e.rect,
		clipDepth: e.clipDepth,
		since: e.since,
		extras: e.extras,
		tags: { placedBy: e.placedBy, matrixBy: e.matrixBy, changedBy: e.changedBy },
		edited: doc.entryEdited(e),
		hidden: doc.hidden.includes(e.depth),
	};
}

function status(doc: SwfDoc) {
	const info = doc.info;
	return {
		movie: {
			path: doc.path,
			index: doc.index,
			object: studio.pkg?.path === doc.path ? studio.pkg.exports[doc.index]?.path : undefined,
			size: info ? { width: info.width, height: info.height } : null,
			frameRate: info?.frameRate,
			frames: info?.frames,
			version: info?.version,
			characters: doc.tree?.characters.length ?? 0,
			texts: info?.texts.length ?? 0,
			scripts: info?.scripts.length ?? 0,
			images: info?.images.length ?? 0,
		},
		place: {
			node: doc.node,
			sprite: doc.sprite,
			trail: doc.trail.map((t) => `${t.label} (${t.id})`).join(" › "),
			character: doc.character
				? {
						id: doc.character.id,
						kind: doc.character.kind,
						label: doc.label(doc.character.id),
					}
				: null,
			frame: doc.frame,
			frames: doc.timeline?.frames.length ?? null,
			objects: doc.entries.length,
			selected: doc.selectedEntry ? entryJson(doc, doc.selectedEntry) : null,
		},
		pending: doc.pending,
		hiddenLayers: doc.hiddenBySprite,
	};
}

function page<T>(items: T[], a: SwfToolArgs, limit = 100) {
	const q = a.query?.toLowerCase();
	const hits = q ? items.filter((x) => JSON.stringify(x).toLowerCase().includes(q)) : items;
	const offset = a.offset ?? 0;
	const n = a.limit ?? limit;
	return { total: hits.length, offset, items: hits.slice(offset, offset + n) };
}

export async function swfTool(a: SwfToolArgs): Promise<unknown> {
	if (a.action === "movies") {
		const here =
			studio.pkg?.exports
				.filter((r) => isMovie(r.class))
				.map((r) => ({
					package: studio.pkg!.path,
					index: r.i,
					path: r.path,
					class: r.class,
				})) ?? [];
		if (!a.query) return { openPackage: here };
		const found = await Promise.all(
			["SwfMovie", "GFxMovieInfo"].map((cls) =>
				rpc("workspace.search", { query: a.query!, class: cls, limit: a.limit ?? 100 }),
			),
		);
		return {
			openPackage: here,
			workspace: found.flatMap((f) => f.results),
			truncated: found.some((f) => f.truncated),
		};
	}

	const doc = await movie(a);
	await goTo(doc, a);

	switch (a.action) {
		case "open":
		case "status": {
			return status(doc);
		}

		case "tree": {
			const chars = (doc.tree?.characters ?? [])
				.filter((c) => !a.kind || c.kind === a.kind)
				.map((c) => ({
					id: c.id,
					kind: c.kind,
					names: c.names,
					tag: c.tag,
					rect: c.rect,
					frames: c.frames,
					deps: c.deps,
					usedBy: c.usedBy,
					import: c.import,
				}));
			return {
				stage: doc.tree?.stage,
				rootFrames: doc.tree?.rootFrames,
				labels: doc.tree?.labels,
				...page(chars, a, 200),
			};
		}

		case "timeline": {
			const t = doc.timeline;
			if (!t || doc.sprite === null) {
				throw new Error("open a sprite or the root timeline (sprite)");
			}
			return {
				sprite: doc.sprite,
				label: spriteLabel(doc),
				frames: t.frames.length,
				frame: doc.frame,
				labels: t.labels,
				actions: t.actions,
				objects: doc.entries.map((e) => entryJson(doc, e)),
			};
		}

		case "info": {
			const info = doc.info;
			if (!info) throw new Error("the movie is not read");
			switch (a.section ?? "texts") {
				case "texts": {
					return page(
						info.texts.map((t) => ({
							...t,
							text: doc.value("text", t.tag, t.text ?? ""),
							edited: doc.isEdited(`text|${t.tag}|0`),
						})),
						a,
					);
				}
				case "scripts": {
					return page(
						info.scripts.map((s) => ({
							...s,
							strings: s.strings.map((x) => ({
								...x,
								text: doc.value("string", s.tag, x.text, x.index),
							})),
						})),
						a,
						30,
					);
				}
				case "images": {
					return page(
						info.images.map((im) => ({
							...im,
							fileName: doc.value("image", im.tag, im.fileName),
						})),
						a,
					);
				}
				case "linkage": {
					return { exports: info.exports, imports: info.imports };
				}
				case "tags": {
					return page(info.tags, a, 200);
				}
			}
			throw new Error(`unknown section ${a.section}`);
		}

		case "script": {
			if (a.tag === undefined) throw new Error("give tag (info section scripts lists them)");
			return rpc("swf.script", { path: doc.path, index: doc.index, tag: a.tag });
		}

		case "edit": {
			const e = entryAt(doc, a.depth);
			const sprite = doc.sprite!;
			doc.depth = e.depth;
			const moves = [a.x, a.y, a.dx, a.dy, a.scaleX, a.scaleY, a.rotation, a.skew];
			if (a.matrix) {
				doc.setMatrix(e, { ...e.matrix, ...a.matrix });
			} else if (moves.some((v) => v !== undefined)) {
				const d = decompose(e.matrix);
				if (a.x !== undefined) d.x = a.x;
				if (a.y !== undefined) d.y = a.y;
				d.x += a.dx ?? 0;
				d.y += a.dy ?? 0;
				if (a.scaleX !== undefined) d.sx = a.scaleX;
				if (a.scaleY !== undefined) d.sy = a.scaleY;
				if (a.rotation !== undefined) d.rot = a.rotation;
				if (a.skew !== undefined) d.skew = a.skew;
				doc.setMatrix(e, compose(d));
			}
			if (a.alpha !== undefined) {
				const base = e.cxform ?? { mul: [1, 1, 1, 1], add: [0, 0, 0, 0] };
				const mul = [...base.mul];
				mul[3] = Math.max(0, Math.min(1, a.alpha / 100));
				const identity = mul.every((m) => m === 1) && base.add.every((v) => v === 0);
				doc.float(e);
				doc.editPlace(sprite, e.changedBy, {
					cxform: identity ? null : { mul, add: base.add },
				});
			}
			if (a.name !== undefined) {
				doc.editPlace(sprite, e.placedBy, {
					name: a.name.trim() === "" ? null : a.name.trim(),
				});
			}
			await doc.settled();
			return { object: entryJson(doc, entryAt(doc, e.depth)), pending: doc.pending.length };
		}

		case "remove": {
			doc.removeEntry(entryAt(doc, a.depth));
			await doc.settled();
			return status(doc);
		}

		case "revert": {
			doc.revertEntry(entryAt(doc, a.depth));
			await doc.settled();
			return { object: entryJson(doc, entryAt(doc, a.depth!)), pending: doc.pending.length };
		}

		case "text": {
			const kind = a.kind ?? "text";
			if (kind !== "text" && kind !== "string" && kind !== "image") {
				throw new Error(
					"kind is text (edit-text field), string (script string) or image (external image file name)",
				);
			}
			if (a.tag === undefined || a.text === undefined) throw new Error("give tag and text");
			if (kind === "string" && a.index === undefined) {
				throw new Error("a script string needs its index");
			}
			doc.editText(kind, a.tag, a.text, kind === "string" ? a.index : undefined);
			await doc.settled();
			return { pending: doc.pending };
		}

		case "layers": {
			if (doc.sprite === null) throw new Error("open a sprite or the root timeline (sprite)");
			if (a.showAll) doc.setHidden(doc.hidden, false);
			if (a.solo) {
				doc.setHidden(
					doc.entries.map((e) => e.depth).filter((d) => !a.solo!.includes(d)),
					true,
				);
				doc.setHidden(a.solo, false);
			}
			if (a.hide) doc.setHidden(a.hide, true);
			if (a.show) doc.setHidden(a.show, false);
			return { sprite: doc.sprite, hidden: doc.hidden };
		}

		case "apply": {
			const n = doc.pending.length;
			if (n === 0) return { applied: 0, note: "nothing is pending" };
			await doc.apply();
			if (doc.pending.length > 0) {
				throw new Error("the edits were not applied (see the window's message)");
			}
			await doc.settled();
			return {
				applied: n,
				undo: studio.undoLabels,
				note: "the package is changed in memory: save it (file.save) or build the mod",
			};
		}

		case "discard": {
			doc.discardEdits();
			await doc.settled();
			return status(doc);
		}
	}
	throw new Error(`unknown action ${a.action as string}`);
}
