<!--
	One Ruffle rendering of a `swf.preview` request, filling its parent. A new
	request (another `key`) loads in a second player behind the visible one,
	which is only replaced once Ruffle has drawn the new movie, so changes
	never flash an empty canvas. Only `key` triggers a load: the request
	object is rebuilt by every refresh. The stage stacks these: the frame
	without its floating objects, and each floating object alone.
-->
<script lang="ts">
	import { untrack } from "svelte";
	import { rpc } from "$lib/studio/rpc";
	import { loadRuffle, registerMovie, type RuffleElement } from "$lib/studio/ruffle";
	import type { SwfDoc } from "$lib/studio/swf.svelte";
	import type { UpkMethods, SwfRect } from "../../../../electron/src/ipc/contract";

	type Request = Omit<UpkMethods["swf.preview"]["params"], "path" | "index">;

	interface Props {
		doc: SwfDoc;
		request: Request;
		/** Identity of the request: a new key loads a new movie. */
		key: string;
		/** Draw over what is beneath (no background colour). */
		transparent?: boolean;
		/** A static picture (layouts): paused once drawn, so idle layers cost nothing. */
		still?: boolean;
		onload?: (r: { rect: SwfRect; missing: string[]; key: string }) => void;
		onerror?: (message: string) => void;
	}
	const {
		doc,
		request,
		key,
		transparent = true,
		still = true,
		onload,
		onerror,
	}: Props = $props();

	let host = $state<HTMLDivElement | null>(null);
	let current: { player: RuffleElement; route: { dispose(): void } } | null = null;
	let generation = 0;

	/** Resolves once Ruffle has parsed the movie (or after 20 s, not to hang a layer). */
	const loaded = (player: RuffleElement) =>
		new Promise<void>((resolve) => {
			if (player.readyState === 2) {
				resolve();
				return;
			}
			player.addEventListener("loadeddata", () => resolve(), { once: true });
			setTimeout(resolve, 20_000);
		});

	/**
	 * Resolves once the movie is drawn. `loadeddata` comes before the shared
	 * libraries it imports (fonts, SharedAssets) are fetched and parsed, and a
	 * layer swapped in or paused then shows an empty frame. With the canvas
	 * renderer the picture can be read: it is done when a thumbnail of it has
	 * not changed for 600 ms (a uniform one, nothing drawn yet but the
	 * background, only after 2.5 s), or after 6 s.
	 */
	async function settled(player: RuffleElement, stale: () => boolean): Promise<void> {
		const probe = document.createElement("canvas");
		probe.width = 48;
		probe.height = 27;
		const ctx = probe.getContext("2d", { willReadFrequently: true });
		const start = performance.now();
		let last = "";
		let since = start;
		while (performance.now() - start < 6000 && !stale()) {
			await new Promise((r) => setTimeout(r, 120));
			const canvas = player.shadowRoot?.querySelector("canvas");
			if (!ctx || !canvas || canvas.width === 0 || canvas.height === 0) continue;
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
			} else if (now - since >= 600 && (!uniform || now - start >= 2500)) {
				return;
			}
		}
	}

	const frames = (n: number) =>
		new Promise<void>((resolve) => {
			const step = (left: number) =>
				left <= 0 ? resolve() : requestAnimationFrame(() => step(left - 1));
			step(n);
		});

	/** The key last asked for: an equal request rebuilt by a refresh is not a new movie. */
	let asked = "";
	$effect(() => {
		const k = key;
		if (!host || k === asked) return;
		asked = k;
		void show(untrack(() => $state.snapshot(request)) as Request, k);
	});

	// A paused player redraws at a new size only while ticking: wake the
	// visible one briefly whenever the layer is resized (zoom, panes).
	$effect(() => {
		if (!host || !still) return;
		let timer: ReturnType<typeof setTimeout> | null = null;
		const observer = new ResizeObserver(() => {
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
			current?.player.remove();
			current?.route.dispose();
			current = null;
		};
	});

	async function show(r: Request, k: string): Promise<void> {
		const gen = ++generation;
		try {
			const [preview, libraries, ruffle] = await Promise.all([
				rpc("swf.preview", { path: doc.path, index: doc.index, ...r }),
				doc.librariesOnce(),
				loadRuffle(),
			]);
			if (gen !== generation || !host) return;
			const route = registerMovie(preview.swf, libraries.libraries);
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
				url: route.url,
				scale: "exactFit",
				letterbox: "off",
				wmode: transparent ? "transparent" : "opaque",
				backgroundColor: transparent ? "#00000000" : null,
				contextMenu: "off",
				// A 2D canvas keeps its picture while the player is paused (a
				// WebGL one goes blank), which is what lets layouts stop ticking.
				...(still ? { preferredRenderer: "canvas" } : {}),
			});
			// `load` resolves when loading starts; the movie is parsed when
			// `loadeddata` fires, and drawn on a frame after that.
			await loaded(player);
			await frames(4);
			if (still) await settled(player, () => gen !== generation);
			if (gen !== generation) {
				player.remove();
				route.dispose();
				return;
			}
			player.style.opacity = "1";
			const old = current;
			current = { player, route };
			old?.player.remove();
			old?.route.dispose();
			// A layout never changes by itself: once drawn, stop ticking (a
			// playing layer costs a third of a core, a paused one nothing).
			if (still) player.pause();
			onload?.({ rect: preview.rect, missing: preview.missing, key: k });
		} catch (error) {
			if (gen !== generation) return;
			onerror?.(error instanceof Error ? error.message : String(error));
		}
	}
</script>

<div bind:this={host} class="pointer-events-none absolute inset-0"></div>
