<!--
	One Ruffle rendering of a `swf.preview` request, filling its parent. A new
	request (another `key`) loads in a second player behind the visible one,
	which is only replaced once Ruffle has drawn the new movie, so changes
	never flash an empty canvas. Only `key` triggers a load: the request
	object is rebuilt by every refresh; an empty key waits. The stage stacks
	these: the frame without its floating objects, and each floating object
	alone.

	A still layer shows a picture it already has without Ruffle
	(`$lib/studio/swf-render`): one kept in memory for this request (going
	back up a clip, an undo), one being rendered ahead (hovering in the tree),
	or one kept on disk for the same preview bytes (an earlier session).
	Ruffle only runs again when the layer has grown past the picture's
	resolution.
-->
<script lang="ts">
	import { untrack } from "svelte";
	import { rpc } from "$lib/studio/rpc";
	import { registerMovie, type RuffleElement } from "$lib/studio/ruffle";
	import {
		contentKey,
		copy,
		loaded,
		loadPicture,
		mountPlayer,
		pendingPicture,
		pictureKey,
		previewLibraries,
		recall,
		remember,
		settled,
		storePicture,
		type Picture,
		type Request,
	} from "$lib/studio/swf-render";
	import type { SwfDoc } from "$lib/studio/swf.svelte";
	import type { SwfRect } from "../../../../electron/src/ipc/contract";

	interface Props {
		doc: SwfDoc;
		request: Request;
		/** Identity of the request: a new key loads a new movie (empty: nothing yet). */
		key: string;
		/** Draw over what is beneath (no background colour). */
		transparent?: boolean;
		/** A static picture (layouts): paused once drawn, so idle layers cost nothing. */
		still?: boolean;
		onload?: (r: { rect: SwfRect; missing: string[]; key: string }) => void;
		onerror?: (message: string) => void;
		/** A new movie started loading (true) or is on screen / failed (false). */
		onbusy?: (busy: boolean) => void;
	}
	const {
		doc,
		request,
		key,
		transparent = true,
		still = true,
		onload,
		onerror,
		onbusy,
	}: Props = $props();

	let host = $state<HTMLDivElement | null>(null);
	/** What is on screen: a Ruffle player, or a remembered picture (no player). */
	let current: {
		el: HTMLElement;
		player?: RuffleElement;
		route?: { dispose(): void };
		/** A picture's resolution. */
		width?: number;
	} | null = null;
	let generation = 0;

	const frames = (n: number) =>
		new Promise<void>((resolve) => {
			const step = (left: number) =>
				left <= 0 ? resolve() : requestAnimationFrame(() => step(left - 1));
			step(n);
		});

	/** The key last asked for: an equal request rebuilt by a refresh is not a new movie. */
	let asked = "";
	let askedRequest: Request | null = null;
	$effect(() => {
		const k = key;
		if (!host || !k || k === asked) return;
		asked = k;
		askedRequest = untrack(() => $state.snapshot(request)) as Request;
		void show(askedRequest, k);
	});

	/**
	 * Device pixels across the layer, as the resize observer last saw it (0
	 * until then). Reading `clientWidth` here would force a layout of the
	 * whole window in the middle of mounting the editor.
	 */
	let seenWidth = 0;
	const hostWidth = () => Math.round(seenWidth * devicePixelRatio);

	// A paused player redraws at a new size only while ticking: wake the
	// visible one briefly whenever the layer is resized (zoom, panes). A
	// remembered picture grown past its resolution is rendered again.
	$effect(() => {
		if (!host || !still) return;
		let timer: ReturnType<typeof setTimeout> | null = null;
		const observer = new ResizeObserver((entries) => {
			seenWidth = entries.at(-1)?.contentRect.width ?? seenWidth;
			if (current && !current.player) {
				if (askedRequest && hostWidth() > (current.width ?? 0) * 1.02) {
					void show(askedRequest, asked, true);
				}
				return;
			}
			const player = current?.player;
			if (!player || player.readyState !== 2) return;
			player.play();
			if (timer) clearTimeout(timer);
			timer = setTimeout(() => {
				if (current?.player === player) player.pause();
			}, 300);
		});
		observer.observe(host);
		return () => {
			observer.disconnect();
			if (timer) clearTimeout(timer);
		};
	});

	$effect(() => {
		return () => {
			generation++;
			current?.el.remove();
			current?.route?.dispose();
			current = null;
		};
	});

	function swap(next: NonNullable<typeof current>): void {
		const old = current;
		current = next;
		old?.el.remove();
		old?.route?.dispose();
	}

	/** Put a picture on screen; false when it is too small for the layer. */
	function showPicture(picture: Picture, k: string): boolean {
		if (!host) return true;
		const el = copy(picture.canvas);
		Object.assign(el.style, {
			position: "absolute",
			inset: "0",
			width: "100%",
			height: "100%",
			pointerEvents: "none",
		});
		host.append(el);
		swap({ el, width: el.width });
		onload?.({ rect: picture.rect, missing: picture.missing, key: k });
		onbusy?.(false);
		return hostWidth() <= el.width * 1.02;
	}

	/** `sharpen`: render the request again although a picture of it is on screen. */
	async function show(r: Request, k: string, sharpen = false): Promise<void> {
		const gen = ++generation;
		const pk = pictureKey(doc, transparent, k);
		let behind = sharpen;
		if (still && !sharpen) {
			const known = recall(pk) ?? (await pendingPicture(pk));
			if (gen !== generation) return;
			if (known) {
				if (showPicture(known, k)) return;
				behind = true;
			}
		}
		if (!behind) onbusy?.(true);
		try {
			const preview = await rpc("swf.preview", { path: doc.path, index: doc.index, ...r });
			const libraries = await previewLibraries(doc, preview);
			if (gen !== generation || !host) return;
			const ck = still ? await contentKey(preview.swf, libraries, transparent) : "";
			if (ck && !behind) {
				const stored = await loadPicture(ck, hostWidth());
				if (gen !== generation) return;
				if (stored) {
					remember(pk, stored);
					if (showPicture(stored, k)) return;
					behind = true;
				}
			}
			const route = registerMovie(preview.swf, libraries);
			const player = await mountPlayer(host, route.url, transparent, still);
			// `load` resolves when loading starts; the movie is parsed when
			// `loadeddata` fires, and drawn on a frame after that.
			await loaded(player);
			let complete = false;
			if (still) complete = await settled(player, () => gen !== generation, preview, route);
			else await frames(4);
			if (gen !== generation) {
				player.remove();
				route.dispose();
				return;
			}
			player.style.opacity = "1";
			swap({ el: player, player, route });
			// A picture not seen complete keeps playing (it may still appear)
			// and is not kept.
			if (complete) {
				// A layout never changes by itself: once drawn, stop ticking (a
				// playing layer costs a third of a core, a paused one nothing).
				player.pause();
				const canvas = player.shadowRoot?.querySelector("canvas");
				if (canvas && canvas.width > 0) {
					const picture = {
						canvas: copy(canvas),
						rect: preview.rect,
						missing: preview.missing,
					};
					remember(pk, picture);
					storePicture(ck, picture);
				}
			}
			onload?.({ rect: preview.rect, missing: preview.missing, key: k });
			onbusy?.(false);
		} catch (error) {
			if (gen !== generation) return;
			onbusy?.(false);
			// The remembered picture stays on screen.
			if (behind) return;
			onerror?.(error instanceof Error ? error.message : String(error));
		}
	}
</script>

<div bind:this={host} class="pointer-events-none absolute inset-0"></div>
