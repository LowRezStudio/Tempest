// What the movie editor exports. Vectors and files come from the backend
// (`swf.export`: SVG of a character or a frame, TrueType of a font, MP3/WAV
// of a sound, binary data as is); pictures are rendered by Ruffle, the same
// player as the stage, so PNGs show texts, filters and blend modes too: a
// hidden player with the 2D canvas renderer draws the target, and once the
// picture has settled the canvas is copied at the requested scale.
import { invoke, pickDirectory, pickSave } from "$lib/platform/api";
import { bytes } from "./format";
import { rpc } from "./rpc";
import { loadRuffle, registerMovie, type RuffleElement } from "./ruffle";
import { studio } from "./state.svelte";
import { toasts } from "./toast.svelte";
import type { SwfPreviewTarget, SwfRect } from "../../../electron/src/ipc/contract";
import type { SwfDoc } from "./swf.svelte";

const tick = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Resolves once Ruffle has parsed the movie (or after 20 s). */
function parsed(player: RuffleElement): Promise<void> {
	return new Promise((resolve) => {
		if (player.readyState === 2) {
			resolve();
			return;
		}
		player.addEventListener("loadeddata", () => resolve(), { once: true });
		setTimeout(resolve, 20_000);
	});
}

/**
 * Resolves once the picture stops changing: shared libraries (fonts,
 * SharedAssets) load after `loadeddata`, so a frame read too early misses
 * them. Same rule as the stage layers: a 48×27 thumbnail unchanged for
 * 600 ms (a uniform one only after 2.5 s), at most 6 s.
 */
async function settled(player: RuffleElement): Promise<void> {
	const probe = document.createElement("canvas");
	probe.width = 48;
	probe.height = 27;
	const ctx = probe.getContext("2d", { willReadFrequently: true });
	const start = performance.now();
	let last = "";
	let since = start;
	while (performance.now() - start < 6000) {
		await tick(120);
		const canvas = player.shadowRoot?.querySelector("canvas");
		if (!ctx || !canvas || canvas.width === 0 || canvas.height === 0) continue;
		ctx.clearRect(0, 0, probe.width, probe.height);
		ctx.drawImage(canvas, 0, 0, probe.width, probe.height);
		const d = ctx.getImageData(0, 0, probe.width, probe.height).data;
		let hash = 0;
		let uniform = true;
		for (let i = 0; i < d.length; i += 4) {
			hash = (hash * 31 + d[i] + d[i + 1] * 3 + d[i + 2] * 7 + d[i + 3] * 11) | 0;
			if (uniform && (d[i] !== d[0] || d[i + 3] !== d[3])) uniform = false;
		}
		const now = performance.now();
		if (String(hash) !== last) {
			last = String(hash);
			since = now;
		} else if (now - since >= 600 && (!uniform || now - start >= 2500)) {
			return;
		}
	}
}

/**
 * A PNG of a preview target (pending edits included), `scale` × its size in
 * pixels, transparent around it. `rect` fixes the area (frame sequences).
 */
export async function renderPng(
	doc: SwfDoc,
	target: SwfPreviewTarget,
	scale = 1,
	rect?: SwfRect,
): Promise<{ png: Uint8Array; width: number; height: number }> {
	const [preview, libraries, ruffle] = await Promise.all([
		rpc("swf.preview", { path: doc.path, index: doc.index, target, edits: doc.pending, rect }),
		doc.librariesOnce(),
		loadRuffle(),
	]);
	// A fixed area gives every frame of a sequence the same size.
	const area = rect ?? preview.rect;
	const width = Math.max(1, Math.round(area.w * scale));
	const height = Math.max(1, Math.round(area.h * scale));
	if (width * height > 64_000_000) {
		throw new Error(`${width}×${height} is too large: lower the scale`);
	}
	const dpr = window.devicePixelRatio || 1;
	const host = document.createElement("div");
	Object.assign(host.style, {
		position: "fixed",
		left: "0",
		top: "0",
		width: `${width / dpr}px`,
		height: `${height / dpr}px`,
		opacity: "0",
		pointerEvents: "none",
		zIndex: "-1",
	});
	document.body.append(host);
	const route = registerMovie(preview.swf, libraries.libraries);
	const player = ruffle.newest().createPlayer();
	const style = document.createElement("style");
	style.textContent = "#play-button { display: none !important; }";
	player.shadowRoot?.append(style);
	Object.assign(player.style, { width: "100%", height: "100%" });
	host.append(player);
	try {
		await player.ruffle().load({
			url: route.url,
			scale: "exactFit",
			letterbox: "off",
			wmode: "transparent",
			backgroundColor: "#00000000",
			contextMenu: "off",
			preferredRenderer: "canvas",
		});
		await parsed(player);
		await settled(player);
		const canvas = player.shadowRoot?.querySelector("canvas");
		if (!canvas || canvas.width === 0) throw new Error("Ruffle drew nothing");
		const out = document.createElement("canvas");
		out.width = width;
		out.height = height;
		out.getContext("2d")?.drawImage(canvas, 0, 0, width, height);
		const blob = await new Promise<Blob>((resolve, reject) =>
			out.toBlob((b) => (b ? resolve(b) : reject(new Error("no PNG"))), "image/png"),
		);
		return { png: new Uint8Array(await blob.arrayBuffer()), width, height };
	} finally {
		player.remove();
		route.dispose();
		host.remove();
	}
}

const safe = (s: string) => s.replaceAll(/[^\w.-]+/g, "_").replaceAll(/^_+|_+$/g, "") || "movie";

/** A file name stem for a target: linkage name or kind and id, plus the frame. */
export function stemOf(doc: SwfDoc, target: SwfPreviewTarget): string {
	const movie = studio.pkg?.exports[doc.index]?.name ?? "movie";
	if (target.kind === "root") return safe(`${movie}_frame${target.frame + 1}`);
	const label = safe(doc.label(target.id));
	return target.kind === "frame"
		? safe(`${movie}_${label}_frame${target.frame + 1}`)
		: safe(`${movie}_${label}`);
}

function reportWarnings(warnings: string[]) {
	if (warnings.length > 0) toasts.push("warning", warnings.slice(0, 6).join("\n"), "Export");
}

/** Save a target as SVG. */
export async function exportSvg(doc: SwfDoc, target: SwfPreviewTarget): Promise<void> {
	const out = await pickSave("Export SVG", `${stemOf(doc, target)}.svg`, [
		{ name: "SVG", extensions: ["svg"] },
	]);
	if (!out) return;
	try {
		const r = await studio.run("Writing SVG…", () =>
			rpc("swf.export", {
				path: doc.path,
				index: doc.index,
				format: "svg",
				target,
				out,
				edits: doc.pending,
			}),
		);
		toasts.push("success", `${bytes(r.bytes)} written`, r.out);
		reportWarnings(r.warnings);
	} catch (error) {
		toasts.error(error, "Could not export the SVG");
	}
}

/** Save a target as PNG, rendered at `scale`. */
export async function exportPng(
	doc: SwfDoc,
	target: SwfPreviewTarget,
	scale: number,
): Promise<void> {
	const out = await pickSave("Export PNG", `${stemOf(doc, target)}.png`, [
		{ name: "PNG", extensions: ["png"] },
	]);
	if (!out) return;
	try {
		const r = await studio.run("Rendering…", () => renderPng(doc, target, scale));
		await invoke("fs:write-bytes", { path: out, bytes: r.png });
		toasts.push("success", `${r.width}×${r.height}, ${bytes(r.png.length)}`, out);
	} catch (error) {
		toasts.error(error, "Could not render the PNG");
	}
}

/**
 * Every frame of a sprite (0: the root timeline) as numbered PNGs in a
 * folder, all with the same area (the union of the frames' bounds) so they
 * line up as an animation. Each frame is the static layout of that frame.
 */
export async function exportFrames(doc: SwfDoc, sprite: number, scale: number): Promise<void> {
	const dir = await pickDirectory("Folder for the frames");
	if (!dir) return;
	const stem =
		sprite === 0
			? safe(studio.pkg?.exports[doc.index]?.name ?? "movie")
			: stemOf(doc, { kind: "character", id: sprite });
	try {
		const t = await rpc("swf.timeline", {
			path: doc.path,
			index: doc.index,
			sprite,
			edits: doc.pending,
		});
		let rect: SwfRect | undefined;
		if (sprite === 0 && doc.tree) {
			rect = doc.tree.stage;
		} else {
			for (const list of t.frames) {
				for (const e of list) {
					if (!e.rect || e.clipDepth !== null) continue;
					const r = e.rect;
					rect = rect
						? (() => {
								const x = Math.min(rect!.x, r.x);
								const y = Math.min(rect!.y, r.y);
								return {
									x,
									y,
									w: Math.max(rect!.x + rect!.w, r.x + r.w) - x,
									h: Math.max(rect!.y + rect!.h, r.y + r.h) - y,
								};
							})()
						: r;
				}
			}
		}
		const n = t.frames.length;
		const digits = String(n).length;
		let written = 0;
		for (let f = 0; f < n; f++) {
			studio.busy = `Rendering frame ${f + 1} of ${n}…`;
			const target: SwfPreviewTarget =
				sprite === 0 ? { kind: "root", frame: f } : { kind: "frame", id: sprite, frame: f };
			const r = await renderPng(doc, target, scale, rect);
			await invoke("fs:write-bytes", {
				path: `${dir}/${stem}_${String(f + 1).padStart(digits, "0")}.png`,
				bytes: r.png,
			});
			written++;
		}
		toasts.push("success", `${written} frame${written === 1 ? "" : "s"} written`, dir);
	} catch (error) {
		toasts.error(error, "Could not render the frames");
	} finally {
		studio.busy = null;
	}
}

/** Save a font (TrueType), sound (MP3/WAV) or binary data as a file. */
export async function exportFile(
	doc: SwfDoc,
	id: number,
	format: "ttf" | "sound" | "binary",
	ext: string,
): Promise<void> {
	const stem = stemOf(doc, { kind: "character", id });
	const out = await pickSave("Export", `${stem}.${ext}`, [
		{ name: ext.toUpperCase(), extensions: [ext] },
	]);
	if (!out) return;
	try {
		const r = await studio.run("Exporting…", () =>
			rpc("swf.export", {
				path: doc.path,
				index: doc.index,
				format,
				id,
				out,
				edits: doc.pending,
			}),
		);
		toasts.push("success", `${bytes(r.bytes)} written`, r.out);
		reportWarnings(r.warnings);
	} catch (error) {
		toasts.error(error, "Could not export");
	}
}
