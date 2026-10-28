// The movie editor's state, one document per SwfMovie export: the character
// dictionary, the tag view, the timeline of the sprite being looked at, the
// selection and the edits not yet written into the package. Edits stay in
// the renderer until Apply; every backend view (tree, timeline, preview)
// takes them as a parameter, so what is shown always includes them.
// Documents outlive their component: an undo re-selects the export, which
// remounts the editor, and the user's place must survive that.
import { pickFile, pickSave } from "$lib/platform/api";
import { bytes, safeName } from "./format";
import { rpc } from "./rpc";
import { studio } from "./state.svelte";
import { toasts } from "./toast.svelte";
import type {
	SwfCharacter,
	SwfEdit,
	SwfEditTextProps,
	SwfEntry,
	SwfImage,
	SwfInfo,
	SwfLibrary,
	SwfMatrix,
	SwfTimeline,
	SwfTree,
	UpkMethods,
} from "../../../electron/src/ipc/contract";

/** What the editor shows: the movie, the root timeline, a character or a tag list. */
export type SwfNode =
	| "movie"
	| "frames"
	| "texts"
	| "scripts"
	| "images"
	| "linkage"
	| "tags"
	| `char:${number}`;

export type SwfGroup = {
	id: string;
	title: string;
	kinds: SwfCharacter["kind"][];
};

/** The tree's character groups, in FFDec's order. */
export const SWF_GROUPS: SwfGroup[] = [
	{ id: "sprites", title: "Sprites", kinds: ["sprite"] },
	{ id: "shapes", title: "Shapes", kinds: ["shape", "morph"] },
	{ id: "images", title: "Images", kinds: ["image", "subimage", "bitmap"] },
	{ id: "texts", title: "Texts", kinds: ["edittext", "text"] },
	{ id: "fonts", title: "Fonts", kinds: ["font"] },
	{ id: "buttons", title: "Buttons", kinds: ["button"] },
	{ id: "others", title: "Sounds, video, data", kinds: ["sound", "video", "binary"] },
	{ id: "imports", title: "Imported", kinds: ["import"] },
];

/** Translation, scale, rotation (degrees) and skew of a matrix. */
export type Decomposed = {
	x: number;
	y: number;
	sx: number;
	sy: number;
	rot: number;
	skew: number;
};

export function decompose(m: SwfMatrix): Decomposed {
	const sx = Math.hypot(m.a, m.b);
	const det = m.a * m.d - m.b * m.c;
	const rot = Math.atan2(m.b, m.a);
	const sy = (det < 0 ? -1 : 1) * Math.hypot(m.c, m.d);
	const skew = Math.atan2(-m.c, m.d) * (det < 0 ? -1 : 1) - rot;
	const deg = (r: number) => {
		let d = (r * 180) / Math.PI;
		while (d > 180) d -= 360;
		while (d <= -180) d += 360;
		return Math.abs(d) < 1e-6 ? 0 : d;
	};
	return { x: m.tx, y: m.ty, sx, sy, rot: deg(rot), skew: deg(skew) };
}

export function compose(d: Decomposed): SwfMatrix {
	const r = (d.rot * Math.PI) / 180;
	const k = r + (d.skew * Math.PI) / 180;
	return {
		a: d.sx * Math.cos(r),
		b: d.sx * Math.sin(r),
		c: -d.sy * Math.sin(k),
		d: d.sy * Math.cos(k),
		tx: d.x,
		ty: d.y,
	};
}

/** `A · B` (B applied first). */
export function mul(A: SwfMatrix, B: SwfMatrix): SwfMatrix {
	return {
		a: A.a * B.a + A.c * B.b,
		b: A.b * B.a + A.d * B.b,
		c: A.a * B.c + A.c * B.d,
		d: A.b * B.c + A.d * B.d,
		tx: A.a * B.tx + A.c * B.ty + A.tx,
		ty: A.b * B.tx + A.d * B.ty + A.ty,
	};
}

export function invert(m: SwfMatrix): SwfMatrix | null {
	const det = m.a * m.d - m.b * m.c;
	if (Math.abs(det) < 1e-12) return null;
	return {
		a: m.d / det,
		b: -m.b / det,
		c: -m.c / det,
		d: m.a / det,
		tx: (m.c * m.ty - m.d * m.tx) / det,
		ty: (m.b * m.tx - m.a * m.ty) / det,
	};
}

export function apply(m: SwfMatrix, x: number, y: number): [number, number] {
	return [m.a * x + m.c * y + m.tx, m.b * x + m.d * y + m.ty];
}

export const translate = (tx: number, ty: number): SwfMatrix => ({
	a: 1,
	b: 0,
	c: 0,
	d: 1,
	tx,
	ty,
});
export const scaling = (sx: number, sy: number): SwfMatrix => ({
	a: sx,
	b: 0,
	c: 0,
	d: sy,
	tx: 0,
	ty: 0,
});
export const rotation = (r: number): SwfMatrix => ({
	a: Math.cos(r),
	b: Math.sin(r),
	c: -Math.sin(r),
	d: Math.cos(r),
	tx: 0,
	ty: 0,
});

/** Twips are the file's unit: keep pixel values on the 1/20 grid. */
export const twips = (v: number) => Math.round(v * 20) / 20;

function editKey(e: SwfEdit): string {
	switch (e.kind) {
		case "place": {
			return `place|${e.sprite}|${e.tag}`;
		}
		case "deletePlace": {
			return `delete|${e.sprite}|${e.tag}`;
		}
		case "shapeColor": {
			return `shapeColor|${e.tag}|${e.key}`;
		}
		case "staticText": {
			return `staticText|${e.tag}|${e.record}`;
		}
		case "shapeSvg":
		case "editText":
		case "fontGlyphs": {
			return `${e.kind}|${e.tag}`;
		}
		default: {
			return `${e.kind}|${e.tag}|${e.index ?? 0}`;
		}
	}
}

export class SwfDoc {
	readonly path: string;
	readonly index: number;
	tree = $state.raw<SwfTree | null>(null);
	info = $state.raw<SwfInfo | null>(null);
	error = $state<string | null>(null);
	loading = $state(false);
	/** Pending edits by key (`place|sprite|tag`, `text|tag|0`…), in the order made. */
	edits = $state<Record<string, SwfEdit>>({});
	/** Bumped whenever the edits change: previews and timelines reload. */
	version = $state(0);
	node = $state<SwfNode>("frames");
	frame = $state(0);
	/** Selected depth on the current timeline. */
	depth = $state<number | null>(null);
	timeline = $state.raw<SwfTimeline | null>(null);
	/** Static layout (no scripts, nested sprites at frame 1) or the sprite playing. */
	play = $state(false);
	/** Preview zoom; 0 fits the view. */
	zoom = $state(0);
	showBounds = $state(true);
	/** Stage tool: move the selection, or free transform (scale and rotate handles). */
	tool = $state<"move" | "transform">("move");
	treeFilter = $state("");
	applying = $state(false);
	/** The Add image dialog is open. */
	addingImage = $state(false);
	private libraries: Promise<{ libraries: SwfLibrary[]; missing: string[] }> | null = null;
	private timelineFor = "";
	/** The reads in flight, for `settled`. */
	private loadTask: Promise<void> | null = null;
	private refreshTask: Promise<void> = Promise.resolve();
	/** What the package looked like when the movie was last read (see `packageStamp`). */
	private readAt = "";
	private timelineTask: Promise<void> = Promise.resolve();
	/** Character to select once the timeline being opened has loaded (going up a level). */
	private reselect: number | null = null;

	constructor(path: string, index: number) {
		this.path = path;
		this.index = index;
	}

	/** The pending edits as plain objects: they travel over IPC, which cannot clone `$state` proxies. */
	get pending(): SwfEdit[] {
		return Object.values($state.snapshot(this.edits)) as SwfEdit[];
	}

	chars = $derived(new Map((this.tree?.characters ?? []).map((c) => [c.id, c])));

	/** The sprite whose timeline is shown: 0 for the root, null when none. */
	sprite = $derived.by((): number | null => {
		if (this.node === "frames") return 0;
		if (this.node.startsWith("char:")) {
			const c = this.chars.get(Number(this.node.slice(5)));
			return c?.kind === "sprite" ? c.id : null;
		}
		return null;
	});

	character = $derived.by((): SwfCharacter | null =>
		this.node.startsWith("char:") ? (this.chars.get(Number(this.node.slice(5))) ?? null) : null,
	);

	/** The display list at the current frame. */
	entries = $derived.by((): SwfEntry[] => {
		const t = this.timeline;
		if (!t || this.sprite === null || t.sprite !== this.sprite) return [];
		return t.frames[Math.min(this.frame, t.frames.length - 1)] ?? [];
	});

	selectedEntry = $derived(this.entries.find((e) => e.depth === this.depth) ?? null);

	/** Where the root places the current sprite (to outline the stage around it). */
	rootPlacement = $derived.by((): SwfEntry | null => {
		if (!this.sprite) return null;
		return this.rootList.find((e) => e.character === this.sprite) ?? null;
	});

	/** The root timeline's first frame, for the stage outline. */
	rootList = $state.raw<SwfEntry[]>([]);

	/**
	 * The sprite that places a character: the root when its first frame does,
	 * else the first sprite using it (a clip reused in several sprites goes up
	 * to the first of them).
	 */
	parentOf(id: number): number | null {
		if (id === 0) return null;
		if (this.rootList.some((e) => e.character === id)) return 0;
		const parent = this.chars
			.get(id)
			?.usedBy.find((p) => this.chars.get(p)?.kind === "sprite" && p !== id);
		return parent ?? null;
	}

	/** Root › … › the open sprite, for the breadcrumb (empty outside sprite views). */
	trail = $derived.by((): { id: number; label: string }[] => {
		const sprite = this.sprite;
		if (sprite === null) return [];
		const out: { id: number; label: string }[] = [];
		let cur: number | null = sprite;
		const seen = new Set<number>();
		while (cur !== null && cur !== 0 && !seen.has(cur) && out.length < 32) {
			seen.add(cur);
			out.unshift({ id: cur, label: this.label(cur) });
			cur = this.parentOf(cur);
		}
		out.unshift({ id: 0, label: "Root" });
		return out;
	});

	/** Open a sprite of the breadcrumb (0: the root), selecting the clip we came from. */
	goTo(id: number): void {
		const from = this.sprite;
		if (id === from) return;
		const child =
			from !== null
				? this.trail[this.trail.findIndex((t) => t.id === id) + 1]?.id
				: undefined;
		this.open(id === 0 ? "frames" : `char:${id}`);
		this.reselect = child ?? null;
		this.selectReselect();
	}

	/** Go up one level of the breadcrumb. */
	up(): void {
		const parent = this.trail.at(-2);
		if (parent) this.goTo(parent.id);
	}

	/** Edit inside a placed clip (sprites only): the stage's double-click. */
	enter(entry: SwfEntry): boolean {
		if (this.chars.get(entry.character)?.kind !== "sprite") return false;
		this.openCharacter(entry.character);
		return true;
	}

	private selectReselect(): void {
		if (this.reselect === null) return;
		const e = this.entries.find((x) => x.character === this.reselect);
		if (e) {
			this.depth = e.depth;
			this.reselect = null;
		}
	}

	/** Label of a character: its linkage name, or kind and id. */
	label(id: number): string {
		const c = this.chars.get(id);
		if (!c) return `#${id}`;
		return c.names[0] ?? `${c.kind} ${id}`;
	}

	load(): Promise<void> {
		this.loadTask = this.read();
		return this.loadTask;
	}

	/** Resolves once the movie is read and every view includes the pending edits (the MCP tool waits on it). */
	async settled(): Promise<void> {
		await (this.loadTask ?? this.load());
		// Undo, discard or another edit of the package since: read the movie again.
		if (this.readAt !== packageStamp()) await this.load();
		let task: Promise<void>;
		do {
			task = this.refreshTask;
			await task;
		} while (task !== this.refreshTask);
		await this.loadTimeline();
	}

	private async read(): Promise<void> {
		this.readAt = packageStamp();
		this.loading = true;
		this.error = null;
		try {
			const edits = this.pending;
			const [tree, info, root] = await Promise.all([
				rpc("swf.tree", { path: this.path, index: this.index, edits }),
				rpc("swf.info", { path: this.path, index: this.index, edits }),
				rpc("swf.timeline", {
					path: this.path,
					index: this.index,
					sprite: 0,
					edits,
					limit: 1,
				}),
			]);
			this.tree = tree;
			this.info = info;
			this.rootList = root.frames[0] ?? [];
			if (this.node.startsWith("char:") && !this.chars.has(Number(this.node.slice(5)))) {
				this.node = "frames";
			}
			this.timelineFor = "";
			// Tag indices change on Apply: layers start over.
			this.floating = {};
			await this.loadTimeline();
		} catch (error) {
			this.error = error instanceof Error ? error.message : String(error);
		} finally {
			this.loading = false;
		}
	}

	/** Reload the current sprite's timeline when the sprite or the edits changed. */
	loadTimeline(): Promise<void> {
		const sprite = this.sprite;
		if (sprite === null) {
			this.timeline = null;
			return Promise.resolve();
		}
		const key = `${sprite}|${this.version}`;
		if (key === this.timelineFor && this.timeline) return this.timelineTask;
		this.timelineFor = key;
		this.timelineTask = this.readTimeline(sprite, key);
		return this.timelineTask;
	}

	private async readTimeline(sprite: number, key: string): Promise<void> {
		try {
			const t = await rpc("swf.timeline", {
				path: this.path,
				index: this.index,
				sprite,
				edits: this.pending,
			});
			if (this.timelineFor !== key) return;
			this.timeline = t;
			if (this.frame >= t.frames.length) this.frame = Math.max(0, t.frames.length - 1);
			this.selectReselect();
		} catch (error) {
			toasts.error(error, "Could not read the timeline");
		}
	}

	librariesOnce() {
		this.libraries ??= rpc("swf.libraries", { path: this.path, index: this.index }).catch(
			() => ({ libraries: [], missing: [] }),
		);
		return this.libraries;
	}

	open(node: SwfNode): void {
		if (node === this.node) return;
		this.node = node;
		this.frame = 0;
		this.depth = null;
		this.play = false;
		this.reselect = null;
	}

	/** Show a character; sprites open on their timeline. */
	openCharacter(id: number): void {
		this.open(`char:${id}`);
	}

	private changed(): void {
		this.version++;
		this.refreshTask = this.refresh();
	}

	/** Re-read what edits can change: the tree (bounds), texts and the timeline. */
	private async refresh(): Promise<void> {
		const v = this.version;
		try {
			const edits = this.pending;
			const [tree, info] = await Promise.all([
				rpc("swf.tree", { path: this.path, index: this.index, edits }),
				rpc("swf.info", { path: this.path, index: this.index, edits }),
			]);
			if (v !== this.version) return;
			this.tree = tree;
			this.info = info;
			await this.loadTimeline();
		} catch (error) {
			toasts.error(error, "The edit does not apply");
		}
	}

	setEdit(edit: SwfEdit): void {
		this.edits = { ...this.edits, [editKey(edit)]: edit };
		this.changed();
	}

	dropEdit(key: string): void {
		const next = { ...this.edits };
		delete next[key];
		this.edits = next;
		this.changed();
	}

	discardEdits(): void {
		this.edits = {};
		this.changed();
	}

	/** Pending value of a text/string/image edit, else the original. */
	value(
		kind: "text" | "string" | "image",
		tag: number,
		original: string,
		index?: number,
	): string {
		const e = this.edits[`${kind}|${tag}|${index ?? 0}`];
		return e && "text" in e && e.text !== undefined ? e.text : original;
	}

	isEdited(key: string): boolean {
		return key in this.edits;
	}

	editText(kind: "text" | "string" | "image", tag: number, text: string, index?: number): void {
		this.setEdit({ kind, tag, index, text });
	}

	/** Merge a change into the pending edit of a placement tag. */
	editPlace(
		sprite: number,
		tag: number,
		patch: Omit<Extract<SwfEdit, { kind: "place" }>, "kind" | "sprite" | "tag">,
	): void {
		const key = `place|${sprite}|${tag}`;
		const prev = this.edits[key] as Extract<SwfEdit, { kind: "place" }> | undefined;
		const next: Extract<SwfEdit, { kind: "place" }> = {
			...prev,
			...patch,
			kind: "place",
			sprite,
			tag,
		};
		if (prev?.matrix && patch.matrix) next.matrix = { ...prev.matrix, ...patch.matrix };
		this.setEdit(next);
	}

	/**
	 * Layers (depths) left out of the stage render, by sprite (0: the root):
	 * scaffolding the game shows or moves at runtime (hidden states, masks,
	 * panels parked off-stage) only clutters a layout. Editor view only,
	 * never written to the movie.
	 */
	hiddenBySprite = $state<Record<number, number[]>>({});
	hidden = $derived(this.sprite !== null ? (this.hiddenBySprite[this.sprite] ?? []) : []);

	/** Show or hide layers of the current timeline in the render. */
	setHidden(depths: number[], hide: boolean): void {
		if (this.sprite === null) return;
		const cur = new Set(this.hiddenBySprite[this.sprite] ?? []);
		for (const d of depths) {
			if (hide) cur.add(d);
			else cur.delete(d);
		}
		this.hiddenBySprite = {
			...this.hiddenBySprite,
			[this.sprite]: [...cur].sort((a, b) => a - b),
		};
		if (hide && this.depth !== null && cur.has(this.depth)) this.depth = null;
	}

	/**
	 * Objects of the current timeline frame drawn on layers of their own
	 * (by `sprite|frame`): an object gets one when it is first edited and
	 * keeps it, so moving, scaling or fading it never re-renders the frame.
	 */
	floating = $state<Record<string, number[]>>({});
	floatKey = $derived(`${this.sprite}|${this.frame}`);
	floated = $derived(this.floating[this.floatKey] ?? []);

	/** Give an object its own layer (before editing its placement). */
	float(entry: SwfEntry): void {
		const k = this.floatKey;
		const list = this.floating[k] ?? [];
		if (list.includes(entry.depth)) return;
		// A handful at most: each layer is a Ruffle player.
		this.floating = { ...this.floating, [k]: [...list, entry.depth].slice(-8) };
	}

	/** Move an object of the current timeline by a pixel offset. */
	moveEntry(entry: SwfEntry, dx: number, dy: number): void {
		if (this.sprite === null) return;
		this.float(entry);
		this.editPlace(this.sprite, entry.matrixBy, {
			matrix: { tx: twips(entry.matrix.tx + dx), ty: twips(entry.matrix.ty + dy) },
		});
	}

	setMatrix(entry: SwfEntry, matrix: SwfMatrix): void {
		if (this.sprite === null) return;
		this.float(entry);
		const m = { ...matrix, tx: twips(matrix.tx), ty: twips(matrix.ty) };
		this.editPlace(this.sprite, entry.matrixBy, { matrix: m });
	}

	/** Every tag of the current stream that placed or changed this object. */
	tagsOf(entry: SwfEntry): number[] {
		const tags = new Set<number>();
		for (const list of this.timeline?.frames ?? []) {
			for (const e of list) {
				if (e.depth !== entry.depth || e.placedBy !== entry.placedBy) continue;
				tags.add(e.placedBy);
				tags.add(e.matrixBy);
				tags.add(e.changedBy);
			}
		}
		return [...tags].sort((a, b) => a - b);
	}

	/** Remove the object from the timeline: its placing tag and every later change of it. */
	removeEntry(entry: SwfEntry): void {
		if (this.sprite === null) return;
		const next = { ...this.edits };
		for (const tag of this.tagsOf(entry)) {
			delete next[`place|${this.sprite}|${tag}`];
			const d: SwfEdit = { kind: "deletePlace", sprite: this.sprite, tag };
			next[editKey(d)] = d;
		}
		this.edits = next;
		this.depth = null;
		this.changed();
	}

	/** Pending edits touching an object of the current timeline. */
	entryEdited(entry: SwfEntry): boolean {
		if (this.sprite === null) return false;
		return this.tagsOf(entry).some((t) => this.isEdited(`place|${this.sprite}|${t}`));
	}

	/** Drop the pending edits of one object. */
	revertEntry(entry: SwfEntry): void {
		if (this.sprite === null) return;
		const next = { ...this.edits };
		for (const tag of this.tagsOf(entry)) delete next[`place|${this.sprite}|${tag}`];
		this.edits = next;
		this.changed();
	}

	// ----- shapes, texts, fonts ----------------------------------------------------

	/** A shape colour (`#rrggbbaa`) or line width (pixels, `width:` keys). */
	setShapeStyle(tag: number, key: string, value: string): void {
		this.setEdit({ kind: "shapeColor", tag, key, value });
	}

	/** Redraw a shape from an SVG document (its colour edits no longer apply). */
	replaceShape(tag: number, svg: string, fit: boolean): void {
		const next = { ...this.edits };
		for (const k of Object.keys(next)) if (k.startsWith(`shapeColor|${tag}|`)) delete next[k];
		next[`shapeSvg|${tag}`] = { kind: "shapeSvg", tag, svg, fit };
		this.edits = next;
		this.changed();
	}

	/** Merge edit-text properties into the pending edit of a field. */
	setEditText(tag: number, props: Partial<SwfEditTextProps>): void {
		const prev = this.edits[`editText|${tag}`] as
			| Extract<SwfEdit, { kind: "editText" }>
			| undefined;
		this.setEdit({ kind: "editText", tag, props: { ...prev?.props, ...props } });
	}

	/** A static text record's characters and/or colour. */
	setStaticText(tag: number, record: number, change: { text?: string; color?: string }): void {
		const prev = this.edits[`staticText|${tag}|${record}`] as
			| Extract<SwfEdit, { kind: "staticText" }>
			| undefined;
		this.setEdit({ kind: "staticText", tag, record, ...prev, ...change });
	}

	/** Characters to add to a font from a TrueType/OpenType file. */
	addGlyphs(tag: number, file: string, chars: string): void {
		this.setEdit({ kind: "fontGlyphs", tag, file, chars });
	}

	/** The pending edit of a tag, by its key. */
	editOf<K extends SwfEdit["kind"]>(key: string): Extract<SwfEdit, { kind: K }> | undefined {
		return this.edits[key] as Extract<SwfEdit, { kind: K }> | undefined;
	}

	// ----- images -----------------------------------------------------------------

	/**
	 * Changes whenever the package does (an edit, undo, a replaced texture):
	 * part of every render's key, so a new picture of a texture shows at once.
	 */
	get stamp(): number {
		return studio.edits;
	}

	/** The external image a character draws: its own, or a sub-image's atlas. */
	imageOf(id: number): SwfImage | null {
		const images = this.info?.images ?? [];
		const own = images.find((i) => (i.id & 0xffff) === id);
		if (own) return own;
		const c = this.chars.get(id);
		if (c?.kind !== "subimage") return null;
		return images.find((i) => c.deps.includes(i.id & 0xffff)) ?? null;
	}

	/** Pick a PNG/DDS and store it in the image's texture (undoable). */
	async replaceImage(texture: number, image: SwfImage): Promise<void> {
		const file = await pickFile("Replace the image", [
			{ name: "Images", extensions: ["png", "dds"] },
		]);
		if (!file) return;
		try {
			const r = await this.replaceImageWith(texture, image, file);
			toasts.push(
				r.stretched ? "warning" : "success",
				`${r.width}×${r.height} ${r.format}; ${r.note}. Save the package to keep it.`,
				"Image replaced",
			);
		} catch (error) {
			toasts.error(error, "Could not replace the image");
		}
	}

	/** Store a PNG/DDS file in the image's texture (undoable); `stretched`: another aspect ratio. */
	async replaceImageWith(texture: number, image: SwfImage, file: string) {
		const t = await studio.run("Encoding texture…", () =>
			rpc("texture.import", {
				path: this.path,
				index: texture,
				file,
				keepAlpha: true,
				padBlocks: true,
			}),
		);
		studio.markDirty(this.path);
		const ratio = (w: number, h: number) => w / Math.max(1, h);
		const { width: sw, height: sh } = t.source;
		const stretched = Math.abs(ratio(sw, sh) / ratio(image.width, image.height) - 1) > 0.01;
		return {
			width: sw,
			height: sh,
			format: t.format,
			stretched,
			note: `the movie draws it at ${image.width}×${image.height}${stretched ? ", so it is stretched (another aspect ratio)" : ""}`,
		};
	}

	/** Write the image's texture as PNG or DDS. */
	async exportImage(texture: number, image: SwfImage, ext: "png" | "dds"): Promise<void> {
		const stem = image.fileName.replace(/^.*[\\/]/, "").replace(/\.[a-z0-9]+$/i, "");
		const out = await pickSave(`Export ${stem}`, `${safeName(stem)}.${ext}`, [
			{ name: ext.toUpperCase(), extensions: [ext] },
		]);
		if (!out) return;
		try {
			const r = await rpc("texture.export", { path: this.path, index: texture, out });
			toasts.push("success", `${bytes(r.bytes)} written`, r.out);
		} catch (error) {
			toasts.error(error, "Could not export the image");
		}
	}

	/**
	 * Add an external image (and by default a shape drawing it, placed on the
	 * open timeline frame at `place`): pending edits are written with it, as one
	 * undo step. The new object is selected.
	 */
	async addImage(opts: {
		file: string;
		name?: string;
		shape: boolean;
		place?: { x: number; y: number; name?: string };
	}): Promise<UpkMethods["swf.addImage"]["result"] | null> {
		const place =
			opts.shape && opts.place && this.sprite !== null
				? { sprite: this.sprite, frame: this.frame, ...opts.place }
				: undefined;
		try {
			const r = await studio.run("Adding the image…", () =>
				rpc("swf.addImage", {
					path: this.path,
					index: this.index,
					file: opts.file,
					name: opts.name,
					shape: opts.shape,
					place,
					edits: this.pending,
				}),
			);
			studio.markDirty(this.path);
			studio.undoLabels = r.undo;
			this.edits = {};
			this.version++;
			await studio.reloadTables();
			await this.load();
			if (r.placed) {
				this.depth = r.placed.depth;
			} else {
				this.openCharacter(r.shape ?? r.image);
			}
			toasts.push(
				"success",
				`${r.fileName} (${r.width}×${r.height}) → ${r.texture.path}${r.placed ? `, placed at depth ${r.placed.depth}` : ""}. Save the package to keep it.`,
				"Image added",
			);
			return r;
		} catch (error) {
			toasts.error(error, "Could not add the image");
			return null;
		}
	}

	/** Write the pending edits into the package (one undo step). */
	async apply(): Promise<void> {
		const edits = this.pending;
		if (edits.length === 0) return;
		this.applying = true;
		try {
			const r = await rpc("swf.apply", { path: this.path, index: this.index, edits });
			studio.markDirty(this.path);
			studio.undoLabels = r.undo;
			this.edits = {};
			this.version++;
			toasts.push(
				"success",
				`${r.edits} edit${r.edits === 1 ? "" : "s"}, ${r.bytes} bytes; save the package to keep them`,
				"Movie updated",
			);
			await this.load();
		} catch (error) {
			toasts.error(error, "Could not apply the edits");
		} finally {
			this.applying = false;
		}
	}
}

/** The Texture2D of the open package an external image names (`Foo.tga` → `Foo`). */
export function textureFor(fileName: string): number | null {
	const stem = fileName
		.replace(/^.*[\\/]/, "")
		.replace(/\.[a-z0-9]+$/i, "")
		.toLowerCase();
	const row = studio.pkg?.exports.find(
		(e) => e.class.toLowerCase() === "texture2d" && e.name.toLowerCase() === stem,
	);
	return row ? row.i : null;
}

/** Changes when the open package does (an edit, undo or discard). */
function packageStamp(): string {
	return `${studio.edits}|${studio.undoLabels.join("\n")}`;
}

const docs = new Map<string, { doc: SwfDoc; dispose: () => void }>();

/**
 * The document of a movie export, kept across remounts. It is created in a
 * root of its own: a `$derived` belongs to the reactive context that creates
 * it, so a document made inside the editor's `$derived` lost its derived
 * fields (character, sprite, entries) with the first editor, and after a
 * remount the stage kept showing the last character whatever was selected.
 */
export function swfDoc(path: string, index: number): SwfDoc {
	const key = `${path}#${index}`;
	let entry = docs.get(key);
	if (!entry) {
		let doc!: SwfDoc;
		const dispose = $effect.root(() => {
			doc = new SwfDoc(path, index);
		});
		entry = { doc, dispose };
		docs.set(key, entry);
	}
	return entry.doc;
}

/** Forget the documents of a package (closed, discarded). */
export function dropSwfDocs(path: string): void {
	for (const [key, entry] of [...docs]) {
		if (!key.startsWith(`${path}#`)) continue;
		entry.dispose();
		docs.delete(key);
	}
}
