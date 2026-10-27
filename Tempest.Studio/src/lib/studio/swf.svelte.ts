// The movie editor's state, one document per SwfMovie export: the character
// dictionary, the tag view, the timeline of the sprite being looked at, the
// selection and the edits not yet written into the package. Edits stay in
// the renderer until Apply; every backend view (tree, timeline, preview)
// takes them as a parameter, so what is shown always includes them.
// Documents outlive their component: an undo re-selects the export, which
// remounts the editor, and the user's place must survive that.
import { rpc } from "./rpc";
import { studio } from "./state.svelte";
import { toasts } from "./toast.svelte";
import type {
	SwfCharacter,
	SwfEdit,
	SwfEntry,
	SwfInfo,
	SwfLibrary,
	SwfMatrix,
	SwfTimeline,
	SwfTree,
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

/** Twips are the file's unit: keep pixel values on the 1/20 grid. */
export const twips = (v: number) => Math.round(v * 20) / 20;

function editKey(e: SwfEdit): string {
	if (e.kind === "place") return `place|${e.sprite}|${e.tag}`;
	if (e.kind === "deletePlace") return `delete|${e.sprite}|${e.tag}`;
	return `${e.kind}|${e.tag}|${e.index ?? 0}`;
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
	treeFilter = $state("");
	applying = $state(false);
	private libraries: Promise<{ libraries: SwfLibrary[]; missing: string[] }> | null = null;
	private timelineFor = "";

	constructor(path: string, index: number) {
		this.path = path;
		this.index = index;
	}

	get pending(): SwfEdit[] {
		return Object.values(this.edits);
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

	/** Label of a character: its linkage name, or kind and id. */
	label(id: number): string {
		const c = this.chars.get(id);
		if (!c) return `#${id}`;
		return c.names[0] ?? `${c.kind} ${id}`;
	}

	async load(): Promise<void> {
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
			await this.loadTimeline();
		} catch (error) {
			this.error = error instanceof Error ? error.message : String(error);
		} finally {
			this.loading = false;
		}
	}

	/** Reload the current sprite's timeline when the sprite or the edits changed. */
	async loadTimeline(): Promise<void> {
		const sprite = this.sprite;
		if (sprite === null) {
			this.timeline = null;
			return;
		}
		const key = `${sprite}|${this.version}`;
		if (key === this.timelineFor && this.timeline) return;
		this.timelineFor = key;
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
	}

	/** Show a character; sprites open on their timeline. */
	openCharacter(id: number): void {
		this.open(`char:${id}`);
	}

	private changed(): void {
		this.version++;
		void this.refresh();
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
		return e && "text" in e ? e.text : original;
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

	/** Move an object of the current timeline by a pixel offset. */
	moveEntry(entry: SwfEntry, dx: number, dy: number): void {
		if (this.sprite === null) return;
		this.editPlace(this.sprite, entry.matrixBy, {
			matrix: { tx: twips(entry.matrix.tx + dx), ty: twips(entry.matrix.ty + dy) },
		});
	}

	setMatrix(entry: SwfEntry, matrix: SwfMatrix): void {
		if (this.sprite === null) return;
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

const docs = new Map<string, SwfDoc>();

/** The document of a movie export, kept across remounts. */
export function swfDoc(path: string, index: number): SwfDoc {
	const key = `${path}#${index}`;
	let doc = docs.get(key);
	if (!doc) {
		doc = new SwfDoc(path, index);
		docs.set(key, doc);
	}
	return doc;
}

/** Forget the documents of a package (closed, discarded). */
export function dropSwfDocs(path: string): void {
	for (const key of [...docs.keys()]) if (key.startsWith(`${path}#`)) docs.delete(key);
}
