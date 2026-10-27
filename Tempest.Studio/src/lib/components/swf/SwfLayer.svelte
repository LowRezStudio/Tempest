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
		onload?: (r: { rect: SwfRect; missing: string[]; key: string }) => void;
		onerror?: (message: string) => void;
	}
	const { doc, request, key, transparent = true, onload, onerror }: Props = $props();

	let host = $state<HTMLDivElement | null>(null);
	let current: { player: RuffleElement; route: { dispose(): void } } | null = null;
	let generation = 0;

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
			});
			// Ruffle resolves before its first frame is on screen.
			await frames(3);
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
			onload?.({ rect: preview.rect, missing: preview.missing, key: k });
		} catch (error) {
			if (gen !== generation) return;
			onerror?.(error instanceof Error ? error.message : String(error));
		}
	}
</script>

<div bind:this={host} class="pointer-events-none absolute inset-0"></div>
