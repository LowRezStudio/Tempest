// Still pictures of the movie editor's stage, rendered by Ruffle: the check
// that a picture is complete, the pictures kept for later (in memory by
// request, on disk by content) and a queue that renders views before they are
// opened (hovering a clip in the tree or a movie in the export list).
import { rpc } from "./rpc";
import { libraryFetchedAt, loadRuffle, registerMovie, type RuffleElement } from "./ruffle";
import type { SwfRect, UpkMethods } from "../../../electron/src/ipc/contract";
import type { SwfDoc } from "./swf.svelte";

export type Request = Omit<UpkMethods["swf.preview"]["params"], "path" | "index">;
export type Picture = { canvas: HTMLCanvasElement; rect: SwfRect; missing: string[] };

// ----- players -------------------------------------------------------------------

/** The next animation frame (or 50 ms when the window is hidden and frames stop). */
export const tick = () =>
	new Promise<void>((resolve) => {
		const timer = setTimeout(resolve, 50);
		requestAnimationFrame(() => {
			clearTimeout(timer);
			resolve();
		});
	});

/** Resolves once Ruffle has parsed the movie (or after 20 s, not to hang a layer). */
export const loaded = (player: RuffleElement) =>
	new Promise<void>((resolve) => {
		if (player.readyState === 2) {
			resolve();
			return;
		}
		player.addEventListener("loadeddata", () => resolve(), { once: true });
		setTimeout(resolve, 20_000);
	});

/** A hidden player filling `host`, loading `url` the way every stage layer does. */
export async function mountPlayer(
	host: HTMLElement,
	url: string,
	transparent: boolean,
	still: boolean,
): Promise<RuffleElement> {
	const ruffle = await loadRuffle();
	const player = ruffle.newest().createPlayer();
	// Paused layers would show Ruffle's big play button over the movie.
	const style = document.createElement("style");
	style.textContent = "#play-button { display: none !important; }";
	player.shadowRoot?.append(style);
	Object.assign(player.style, {
		position: "absolute",
		inset: "0",
		width: "100%",
		height: "100%",
		opacity: "0",
		pointerEvents: "none",
	});
	host.append(player);
	await player.ruffle().load({
		url,
		scale: "exactFit",
		letterbox: "off",
		wmode: transparent ? "transparent" : "opaque",
		backgroundColor: transparent ? "#00000000" : null,
		contextMenu: "off",
		// A 2D canvas keeps its picture while the player is paused (a
		// WebGL one goes blank), which is what lets layouts stop ticking.
		...(still ? { preferredRenderer: "canvas" } : {}),
	});
	return player;
}

/**
 * Counts what Ruffle draws on a player's 2D canvas: its canvas renderer sets
 * a transform for every shape and bitmap, while a frame of a movie still
 * being parsed is only cleared. Null when the context cannot be watched.
 */
function drawCounter(player: RuffleElement): (() => number) | null {
	const canvas = player.shadowRoot?.querySelector("canvas");
	// The same context Ruffle draws with: a canvas has one 2D context.
	const c = canvas?.getContext("2d");
	if (!c) return null;
	let draws = 0;
	const set = CanvasRenderingContext2D.prototype.setTransform;
	c.setTransform = function (this: CanvasRenderingContext2D, ...args: unknown[]) {
		draws++;
		return (set as (...a: unknown[]) => void).apply(this, args);
	} as typeof c.setTransform;
	return () => draws;
}

/**
 * Resolves once the movie is drawn. `loadeddata` comes long before a large
 * movie is parsed (an 8 MB layout with 213 bitmaps draws after 0.7 s, its
 * frames are blank until then), and before the shared libraries it imports
 * (fonts, SharedAssets) are fetched and parsed. Layout previews carry a
 * transparent pixel (`swf.preview`), so the picture exists once Ruffle has
 * drawn anything; from then on it is done when a thumbnail of it has not
 * changed for 60 ms and no library was fetched for 150 ms, or at once when
 * the preview imports nothing. Without a watchable context, a uniform
 * thumbnail counts only after 500 ms. Gives up after 6 s; false then (or
 * when `stale`): the picture may be incomplete and must not be kept.
 */
export async function settled(
	player: RuffleElement,
	stale: () => boolean,
	preview?: { imports: number },
	route?: { librariesFetched(): number | null },
): Promise<boolean> {
	const probe = document.createElement("canvas");
	probe.width = 48;
	probe.height = 27;
	const ctx = probe.getContext("2d", { willReadFrequently: true });
	const quiet = preview?.imports === 0 ? 1 : 60;
	// Once every library of this movie has been fetched, a library parsed
	// in the next frames is all that can still change the picture; until
	// then (or without a route), any library fetched lately may be ours.
	const librariesQuiet = (now: number) => {
		const fetched = route?.librariesFetched() ?? null;
		return fetched === null ? now - libraryFetchedAt() >= 150 : now - fetched >= 40;
	};
	const start = performance.now();
	let draws: (() => number) | null | undefined;
	let last = "";
	let since = start;
	while (performance.now() - start < 6000 && !stale()) {
		await tick();
		const canvas = player.shadowRoot?.querySelector("canvas");
		if (!ctx || !canvas || canvas.width === 0 || canvas.height === 0) continue;
		draws ??= drawCounter(player);
		if (draws && draws() === 0) continue;
		ctx.clearRect(0, 0, probe.width, probe.height);
		ctx.drawImage(canvas, 0, 0, probe.width, probe.height);
		const d = ctx.getImageData(0, 0, probe.width, probe.height).data;
		let hash = 0;
		let uniform = true;
		for (let i = 0; i < d.length; i += 4) {
			hash = (hash * 31 + d[i] + d[i + 1] * 3 + d[i + 2] * 7 + d[i + 3] * 11) | 0;
			if (
				uniform &&
				(d[i] !== d[0] || d[i + 1] !== d[1] || d[i + 2] !== d[2] || d[i + 3] !== d[3])
			) {
				uniform = false;
			}
		}
		const now = performance.now();
		const key = String(hash);
		if (key !== last) {
			last = key;
			since = now;
		} else if (
			now - since >= quiet &&
			librariesQuiet(now) &&
			(draws || !uniform || now - start >= 500)
		) {
			return true;
		}
	}
	return false;
}

// ----- remembered pictures -------------------------------------------------------

/** Settled pictures by document and request, least recently shown first. */
const pictures = new Map<string, Picture>();
/** About 100 MB of pixels. */
const PICTURE_PIXELS = 25_000_000;
let picturePixels = 0;

const pixels = (c: HTMLCanvasElement) => c.width * c.height;

/** The memory key of a stage request (`key` is the layer's request key). */
export const pictureKey = (doc: SwfDoc, transparent: boolean, key: string) =>
	`${doc.path}#${doc.index}|${transparent}|${key}`;

export function remember(key: string, picture: Picture): void {
	const old = pictures.get(key);
	if (old) {
		picturePixels -= pixels(old.canvas);
		pictures.delete(key);
	}
	pictures.set(key, picture);
	picturePixels += pixels(picture.canvas);
	for (const [k, p] of pictures) {
		if (picturePixels <= PICTURE_PIXELS || k === key) break;
		pictures.delete(k);
		picturePixels -= pixels(p.canvas);
	}
}

export function recall(key: string): Picture | undefined {
	const p = pictures.get(key);
	if (p) {
		pictures.delete(key);
		pictures.set(key, p);
	}
	return p;
}

export function copy(source: CanvasImageSource & { width: number; height: number }) {
	const c = document.createElement("canvas");
	c.width = source.width;
	c.height = source.height;
	c.getContext("2d")?.drawImage(source, 0, 0);
	return c;
}

// ----- on disk, by content ----------------------------------------------------------

// The same preview bytes, libraries and background always draw the same
// picture, so pictures are kept on disk under a digest of exactly that: a
// movie seen in an earlier session, or the same view reached from another
// document, shows without Ruffle.

const hex = (b: ArrayBuffer) =>
	Array.from(new Uint8Array(b), (x) => x.toString(16).padStart(2, "0")).join("");

/** A shared library as Ruffle fetches it. */
type Library = { url: string; swf: Uint8Array };

/** The libraries a preview loads: its own cuts (layouts), else the movie's whole ones. */
export async function previewLibraries(
	doc: SwfDoc,
	preview: { libraries?: Library[] },
): Promise<Library[]> {
	return preview.libraries ?? (await doc.librariesOnce()).libraries;
}

const libraryDigests = new WeakMap<Library[], Promise<string>>();

function librariesDigest(libraries: Library[]): Promise<string> {
	let d = libraryDigests.get(libraries);
	if (!d) {
		d = (async () => {
			const parts = await Promise.all(
				libraries.map(async (l) => {
					const h = await crypto.subtle.digest("SHA-256", l.swf as BufferSource);
					return `${l.url}=${hex(h)}`;
				}),
			);
			return parts.join("|");
		})();
		libraryDigests.set(libraries, d);
	}
	return d;
}

/** The disk key of a rendered preview. */
export async function contentKey(
	swf: Uint8Array,
	libraries: Library[],
	transparent: boolean,
): Promise<string> {
	const [movie, libs] = await Promise.all([
		crypto.subtle.digest("SHA-256", swf as BufferSource),
		librariesDigest(libraries),
	]);
	const all = new TextEncoder().encode(`${hex(movie)}|${libs}|${transparent}`);
	return hex(await crypto.subtle.digest("SHA-256", all));
}

type Stored = {
	key: string;
	blob: Blob;
	width: number;
	rect: SwfRect;
	missing: string[];
	at: number;
};

const STORE = "pictures";
/** Pictures kept on disk; the least recently written go first. */
const DISK_PICTURES = 1500;
let database: Promise<IDBDatabase | null> | null = null;

function db(): Promise<IDBDatabase | null> {
	database ??= new Promise((resolve) => {
		try {
			const open = indexedDB.open("tempest-studio-swf", 1);
			open.onupgradeneeded = () => {
				const store = open.result.createObjectStore(STORE, { keyPath: "key" });
				store.createIndex("at", "at");
			};
			open.onsuccess = () => resolve(open.result);
			open.onerror = () => resolve(null);
		} catch {
			resolve(null);
		}
	});
	return database;
}

const idb = <T>(r: IDBRequest<T>) =>
	new Promise<T>((resolve, reject) => {
		r.onsuccess = () => resolve(r.result);
		r.onerror = () => reject(r.error);
	});

/** A picture kept on disk, if one at least `width` device pixels wide exists. */
export async function loadPicture(key: string, width: number): Promise<Picture | null> {
	try {
		const d = await db();
		if (!d) return null;
		const s = await idb<Stored | undefined>(d.transaction(STORE).objectStore(STORE).get(key));
		if (!s || s.width < width * 0.98) return null;
		const bitmap = await createImageBitmap(s.blob);
		const canvas = copy(bitmap);
		bitmap.close();
		return { canvas, rect: s.rect, missing: s.missing };
	} catch {
		return null;
	}
}

/** Keep a picture on disk (in the background; a narrower one never replaces a wider one). */
export function storePicture(key: string, picture: Picture): void {
	const { canvas, rect, missing } = picture;
	canvas.toBlob(async (blob) => {
		if (!blob) return;
		try {
			const d = await db();
			if (!d) return;
			const store = d.transaction(STORE, "readwrite").objectStore(STORE);
			const old = await idb<Stored | undefined>(store.get(key));
			if (old && old.width > canvas.width) return;
			const t = d.transaction(STORE, "readwrite").objectStore(STORE);
			t.put({
				key,
				blob,
				width: canvas.width,
				rect,
				missing,
				at: Date.now(),
			} satisfies Stored);
			const count = await idb(t.count());
			if (count > DISK_PICTURES) {
				const cursor = t.index("at").openCursor();
				let drop = count - DISK_PICTURES + 100;
				cursor.onsuccess = () => {
					const c = cursor.result;
					if (!c || drop-- <= 0) return;
					c.delete();
					c.continue();
				};
			}
		} catch {
			// A full or unavailable store only costs the next render.
		}
	}, "image/png");
}

// ----- rendering ahead ------------------------------------------------------------

/** Renders started ahead of time, by memory key: a layer asking for one waits for it. */
const ahead = new Map<string, Promise<Picture | null>>();

export const pendingPicture = (key: string) => ahead.get(key);

type Job = {
	doc: SwfDoc;
	request: Request;
	key: string;
	transparent: boolean;
	/** CSS pixels the stage will give the layer. */
	width: number;
	height: number;
	/** Render it (true), or drop it (false). */
	start: (go: boolean) => void;
};
const queue: Job[] = [];
let running = false;

/**
 * Render a stage request in the background and remember its picture, so
 * opening that view shows it at once. Newer requests go first (the last
 * thing hovered is the likeliest click); one renders at a time.
 */
export function prerender(
	doc: SwfDoc,
	request: Request,
	key: string,
	transparent: boolean,
	width: number,
	height: number,
): void {
	const pk = pictureKey(doc, transparent, key);
	if (pictures.has(pk) || ahead.has(pk) || width < 1 || height < 1) return;
	let start!: (go: boolean) => void;
	const started = new Promise<boolean>((resolve) => (start = resolve));
	const job: Job = { doc, request, key, transparent, width, height, start };
	const done = started
		.then((go) => (go ? render(job, pk) : null))
		.catch(() => null)
		.finally(() => ahead.delete(pk));
	ahead.set(pk, done);
	queue.unshift(job);
	// Hovering across a long list must not leave a backlog of stale renders.
	for (const dropped of queue.splice(6)) dropped.start(false);
	void pump();
}

async function pump(): Promise<void> {
	if (running) return;
	running = true;
	while (queue.length > 0) {
		const job = queue.shift()!;
		const done = ahead.get(pictureKey(job.doc, job.transparent, job.key));
		job.start(true);
		await done;
	}
	running = false;
}

async function render(job: Job, pk: string): Promise<Picture | null> {
	const { doc, request, transparent, width, height } = job;
	const dpr = devicePixelRatio;
	const preview = await rpc("swf.preview", { path: doc.path, index: doc.index, ...request });
	const libraries = await previewLibraries(doc, preview);
	const ck = await contentKey(preview.swf, libraries, transparent);
	const stored = await loadPicture(ck, Math.round(width * dpr));
	if (stored) {
		remember(pk, stored);
		return stored;
	}
	const host = document.createElement("div");
	Object.assign(host.style, {
		position: "fixed",
		left: "0",
		top: "0",
		width: `${width}px`,
		height: `${height}px`,
		opacity: "0",
		pointerEvents: "none",
		zIndex: "-1",
	});
	document.body.append(host);
	const route = registerMovie(preview.swf, libraries);
	try {
		const player = await mountPlayer(host, route.url, transparent, true);
		await loaded(player);
		const complete = await settled(player, () => false, preview, route);
		const canvas = player.shadowRoot?.querySelector("canvas");
		if (!complete || !canvas || canvas.width === 0) return null;
		const picture = { canvas: copy(canvas), rect: preview.rect, missing: preview.missing };
		remember(pk, picture);
		storePicture(ck, picture);
		return picture;
	} finally {
		host.remove();
		route.dispose();
	}
}
