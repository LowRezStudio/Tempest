import { invoke } from "$lib/platform/api";
// The movie editor for agents: what the MCP tool `studio_swf` runs in the
// renderer (electron/src/mcp.ts). It drives the same `SwfDoc` as the editor
// (selecting the export mounts it), so the window follows every step, edits
// stay pending until `apply` writes them as one undo step, and the project
// and the cooker see them like any other edit.
import { previewFor } from "./classes";
import { rpc } from "./rpc";
import { studio } from "./state.svelte";
import { renderPng } from "./swf-export";
import { compose, decompose, swfDoc, textureFor, type SwfDoc, type SwfNode } from "./swf.svelte";
import type {
	SwfCharacterKind,
	SwfCxform,
	SwfEditTextProps,
	SwfEntry,
	SwfMatrix,
	SwfPreviewTarget,
} from "../../../electron/src/ipc/contract";

export type SwfToolArgs = {
	action:
		| "movies"
		| "open"
		| "status"
		| "tree"
		| "timeline"
		| "info"
		| "script"
		| "setScript"
		| "edit"
		| "remove"
		| "revert"
		| "text"
		| "layers"
		| "apply"
		| "discard"
		| "addImage"
		| "replaceImage"
		| "exportImage"
		| "character"
		| "shapeStyle"
		| "shapeSvg"
		| "editText"
		| "staticText"
		| "addGlyphs"
		| "export"
		| "compile";
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
	/** A script's key (`info` section scripts lists them). */
	key?: string;
	source?: string;
	pcode?: string;
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
	/** addImage / replaceImage: the PNG or DDS; exportImage: where to write (.png or .dds). */
	file?: string;
	/** addImage: the new Texture2D's name. */
	texture?: string;
	/** addImage: also make a shape drawing it (default true). */
	shape?: boolean;
	/** addImage: place the shape on the open timeline frame (default true). */
	place?: boolean;
	/** edit: a raw colour transform (`mul` and `add`, RGBA; null drops it). */
	cxform?: SwfCxform | null;
	/** shapeStyle: the style key (`fill:g:i`, `stop:g:i:k`, `line:g:i`, `width:g:i`) and its value. */
	styleKey?: string;
	value?: string;
	/** shapeSvg: the SVG document (or `file`); `fit` into the current bounds (default true). */
	svg?: string;
	fit?: boolean;
	/** editText: edit-text properties to change. */
	props?: Partial<SwfEditTextProps>;
	/** staticText: the record and its new characters / colour. */
	record?: number;
	color?: string;
	/** addGlyphs: the characters to add from the font `file`. */
	chars?: string;
	/** export: svg, png (rendered at `scale`), ttf (fonts), sound, binary. */
	format?: "svg" | "png" | "ttf" | "sound" | "binary";
	scale?: number;
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

	if (a.action === "compile") {
		if (a.source === undefined && a.pcode === undefined) {
			throw new Error("give source (ActionScript) or pcode");
		}
		return rpc(
			"swf.compile",
			a.source !== undefined ? { source: a.source } : { pcode: a.pcode! },
		);
	}

	const doc = await movie(a);
	if (a.action !== "character" && a.action !== "export") await goTo(doc, a);
	else if (a.sprite !== undefined || a.node) await goTo(doc, { ...a, character: undefined });

	/** The character an action edits (`character`), with its defining tag. */
	const defined = (want: SwfCharacterKind) => {
		if (a.character === undefined) throw new Error("give character (action tree lists them)");
		const c = doc.tree?.characters.find((x) => x.id === a.character);
		if (!c) throw new Error(`no character ${a.character}`);
		if (c.kind !== want) throw new Error(`character ${c.id} is a ${c.kind}, not a ${want}`);
		if (c.tag === null || c.tag === undefined) {
			throw new Error(`character ${c.id} is imported from another movie`);
		}
		return { id: c.id, tag: c.tag };
	};
	const pendingNow = async () => {
		await doc.settled();
		return { pending: doc.pending.length };
	};

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
						info.images.map((im) => {
							const fileName = doc.value("image", im.tag, im.fileName);
							const texture = textureFor(fileName);
							return {
								...im,
								fileName,
								texture:
									texture === null ? null : studio.pkg?.exports[texture]?.path,
							};
						}),
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

		case "script":
		case "setScript": {
			const scripts = doc.info?.scripts ?? [];
			const sc =
				a.key !== undefined
					? scripts.find((s) => s.key === a.key)
					: scripts.find((s) => s.owner === 0 && s.tag === a.tag && !s.loc.event);
			if (!sc) throw new Error("give key (info section scripts lists them) or a root tag");
			doc.openScript(sc.key);
			if (a.action === "setScript") {
				const mode = a.pcode !== undefined ? "pcode" : "source";
				const text = a.pcode ?? a.source;
				if (text === undefined) throw new Error("give source (ActionScript) or pcode");
				// Compile first: a pending edit that does not compile would fail every view.
				const r = await rpc(
					"swf.compile",
					mode === "source" ? { source: text } : { pcode: text },
				);
				doc.setScript(sc.loc, text, mode);
				await doc.settled();
				return { key: sc.key, bytes: r.bytes, pending: doc.pending.length };
			}
			const r = await rpc("swf.script", {
				path: doc.path,
				index: doc.index,
				loc: sc.loc,
				edits: doc.pending,
			});
			return {
				key: sc.key,
				kind: sc.kind,
				source: r.source,
				pcode: r.pcode,
				problems: r.problems,
				staged: doc.scriptEdit(sc.loc) !== undefined,
			};
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
			if (a.cxform !== undefined) {
				doc.float(e);
				doc.editPlace(sprite, e.changedBy, { cxform: a.cxform });
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

		case "addImage": {
			if (!a.file) throw new Error("give file (a PNG or DDS)");
			const shape = a.shape ?? true;
			const r = await doc.addImage({
				file: a.file,
				name: a.texture,
				shape,
				place:
					shape && (a.place ?? true) && doc.sprite !== null
						? { x: a.x ?? 0, y: a.y ?? 0, name: a.name }
						: undefined,
			});
			if (!r) throw new Error("the image was not added (see the window's message)");
			await doc.settled();
			return {
				...r,
				note: "the package is changed in memory: save it (file.save) or build the mod",
			};
		}

		case "replaceImage":
		case "exportImage": {
			const image =
				a.tag !== undefined
					? (doc.info?.images.find((im) => im.tag === a.tag) ?? null)
					: a.character !== undefined
						? doc.imageOf(a.character)
						: null;
			if (!image) throw new Error("give the image's character or tag (info section images)");
			const texture = textureFor(doc.value("image", image.tag, image.fileName));
			if (texture === null) {
				throw new Error(`no Texture2D for ${image.fileName} in the package`);
			}
			if (!a.file) throw new Error("give file");
			if (a.action === "exportImage") {
				return rpc("texture.export", { path: doc.path, index: texture, out: a.file });
			}
			const r = await doc.replaceImageWith(texture, image, a.file);
			await doc.settled();
			return { ...r, texture: studio.pkg?.exports[texture]?.path };
		}

		case "character": {
			if (a.character === undefined) {
				throw new Error("give character (action tree lists them)");
			}
			doc.openCharacter(a.character);
			await doc.settled();
			const detail = await rpc("swf.character", {
				path: doc.path,
				index: doc.index,
				id: a.character,
				edits: doc.pending,
			});
			return { id: a.character, label: doc.label(a.character), ...detail };
		}

		case "shapeStyle": {
			const c = defined("shape");
			if (!a.styleKey || a.value === undefined) {
				throw new Error("give styleKey and value (action character lists the keys)");
			}
			doc.setShapeStyle(c.tag, a.styleKey, a.value);
			return pendingNow();
		}

		case "shapeSvg": {
			const c = defined("shape");
			const svg =
				a.svg ?? (a.file ? await invoke("fs:read-text", { path: a.file }) : undefined);
			if (!svg) throw new Error("give svg (the document) or file");
			doc.replaceShape(c.tag, svg, a.fit ?? true);
			return pendingNow();
		}

		case "editText": {
			const c = defined("edittext");
			if (!a.props) throw new Error("give props (action character lists them)");
			doc.setEditText(c.tag, a.props);
			return pendingNow();
		}

		case "staticText": {
			const c = defined("text");
			if (a.record === undefined || (a.text === undefined && a.color === undefined)) {
				throw new Error("give record and text and/or color");
			}
			doc.setStaticText(c.tag, a.record, { text: a.text, color: a.color });
			return pendingNow();
		}

		case "addGlyphs": {
			const c = defined("font");
			if (!a.file || !a.chars) throw new Error("give file (TrueType/OpenType) and chars");
			doc.addGlyphs(c.tag, a.file, a.chars);
			return pendingNow();
		}

		case "export": {
			if (!a.file) throw new Error("give file (where to write)");
			const format = a.format ?? "svg";
			if (format === "ttf" || format === "sound" || format === "binary") {
				if (a.character === undefined) throw new Error(`${format} needs character`);
				return rpc("swf.export", {
					path: doc.path,
					index: doc.index,
					format,
					id: a.character,
					out: a.file,
					edits: doc.pending,
				});
			}
			const target: SwfPreviewTarget =
				a.character !== undefined
					? { kind: "character", id: a.character }
					: a.sprite !== undefined && a.sprite !== 0
						? { kind: "frame", id: a.sprite, frame: a.frame ?? 0 }
						: { kind: "root", frame: a.frame ?? 0 };
			if (format === "svg") {
				return rpc("swf.export", {
					path: doc.path,
					index: doc.index,
					format: "svg",
					target,
					out: a.file,
					edits: doc.pending,
				});
			}
			const r = await renderPng(doc, target, a.scale ?? 1);
			await invoke("fs:write-bytes", { path: a.file, bytes: r.png });
			return { out: a.file, width: r.width, height: r.height, bytes: r.png.length };
		}
	}
	throw new Error(`unknown action ${a.action as string}`);
}
