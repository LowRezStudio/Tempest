<script lang="ts">
	import { Maximize2, Pause, Play, RotateCcw, Volume2, VolumeX } from "@lucide/svelte";
	import { rpc } from "$lib/studio/rpc";
	import { loadRuffle, registerMovie, type RuffleElement } from "$lib/studio/ruffle";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let host = $state<HTMLDivElement | null>(null);
	let element = $state<RuffleElement | null>(null);
	let status = $state<"loading" | "ready" | "error">("loading");
	let error = $state<string | null>(null);
	let playing = $state(false);
	let muted = $state(false);
	let reloading = $state(false);
	let libraryCount = $state(0);
	let imageCount = $state(0);
	let missingImages = $state<string[]>([]);
	let missingLibraries = $state<string[]>([]);
	let retry = $state(0);

	// Ruffle is created fresh whenever the movie changes (a reload after an
	// edit remounts the component through SwfPreview's `info` reset).
	$effect(() => {
		const at = path;
		const i = index;
		void retry;
		if (!host) return;
		let cancelled = false;
		let route: { url: string; dispose(): void } | null = null;
		element = null;
		status = "loading";
		error = null;
		playing = false;
		muted = false;
		libraryCount = 0;
		imageCount = 0;
		missingImages = [];
		missingLibraries = [];
		void (async () => {
			try {
				const [movie, libraries] = await Promise.all([
					rpc("swf.player", { path: at, index: i }),
					rpc("swf.libraries", { path: at, index: i }).catch(() => ({
						libraries: [],
						missing: [],
					})),
				]);
				const ruffle = await loadRuffle();
				if (cancelled || !host) return;
				route = registerMovie(movie.swf, libraries.libraries);
				const player = ruffle.newest().createPlayer();
				player.style.width = "100%";
				player.style.height = "100%";
				host.replaceChildren(player);
				await player.ruffle().load({ url: route.url });
				if (cancelled) return;
				element = player;
				playing = player.isPlaying;
				libraryCount = libraries.libraries.length;
				imageCount = movie.images;
				missingImages = movie.missing;
				missingLibraries = libraries.missing ?? [];
				status = "ready";
			} catch (error_) {
				if (cancelled) return;
				error = error_ instanceof Error ? error_.message : String(error_);
				status = "error";
			}
		})();
		return () => {
			cancelled = true;
			route?.dispose();
			element = null;
			host?.replaceChildren();
		};
	});

	function togglePlay() {
		if (!element) return;
		if (element.isPlaying) element.pause();
		else element.play();
		playing = element.isPlaying;
	}

	function toggleMute() {
		if (!element) return;
		muted = !muted;
		element.volume = muted ? 0 : 1;
	}

	async function reload() {
		if (!element) return;
		reloading = true;
		try {
			await element.reload();
			playing = element.isPlaying;
		} finally {
			reloading = false;
		}
	}

	function fullscreen() {
		// Ruffle enlarges its own element; Electron allows it on the click.
		void element?.requestFullscreen();
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-0.5 pb-1">
		<button
			class="btn btn-ghost btn-xs"
			title={playing ? "Pause" : "Play"}
			disabled={status !== "ready"}
			onclick={togglePlay}
		>
			{#if playing}<Pause size={12} />{:else}<Play size={12} />{/if}
		</button>
		<button
			class="btn btn-ghost btn-xs"
			title={muted ? "Unmute" : "Mute"}
			disabled={status !== "ready"}
			onclick={toggleMute}
		>
			{#if muted}<VolumeX size={12} />{:else}<Volume2 size={12} />{/if}
		</button>
		<button
			class="btn btn-ghost btn-xs"
			title="Reload the movie"
			disabled={status !== "ready" || reloading}
			onclick={reload}
		>
			{#if reloading}
				<span class="loading loading-xs loading-spinner"></span>
			{:else}
				<RotateCcw size={12} />
			{/if}
		</button>
		<button
			class="btn btn-ghost btn-xs"
			title="Fullscreen"
			disabled={status !== "ready"}
			onclick={fullscreen}
		>
			<Maximize2 size={12} />
		</button>
		<span class="ml-1 self-center font-mono text-[10px] opacity-50">
			{#if status === "loading"}
				loading Ruffle…
			{:else if status === "error"}
				Ruffle failed
			{:else if libraryCount > 0 || imageCount > 0}
				{libraryCount > 0
					? `${libraryCount} shared librar${libraryCount === 1 ? "y" : "ies"}`
					: ""}{libraryCount > 0 && imageCount > 0 ? " · " : ""}{imageCount > 0
					? `${imageCount} image${imageCount === 1 ? "" : "s"}`
					: ""}
			{/if}
			{#if status === "ready" && missingImages.length > 0}
				<span class="text-warning ml-1" title={missingImages.join("\n")}
					>{missingImages.length} missing</span
				>
			{/if}
			{#if status === "ready" && missingLibraries.length > 0}
				<span class="text-warning ml-1" title={missingLibraries.join("\n")}
					>{missingLibraries.length} import{missingLibraries.length === 1 ? "" : "s"}
					missing</span
				>
			{/if}
		</span>
	</div>
	<div class="relative min-h-0 flex-1 overflow-hidden rounded bg-black">
		<div bind:this={host} class="absolute inset-0"></div>
		{#if status === "ready" && missingLibraries.length > 0}
			<div
				class="bg-warning/90 text-warning-content absolute inset-x-0 bottom-0 truncate px-2 py-1 font-mono text-[10px]"
				title={missingLibraries.join("\n")}
			>
				Missing imported movie{missingLibraries.length === 1 ? "" : "s"}:
				{missingLibraries.map((url) => url.split("/").pop() || url).join(", ")}. Those
				symbols stay blank.
			</div>
		{/if}
		{#if status === "loading"}
			<div
				class="pointer-events-none absolute inset-0 flex items-center justify-center gap-2 text-xs opacity-70"
			>
				<span class="loading loading-sm loading-spinner"></span> Loading Ruffle…
			</div>
		{:else if status === "error"}
			<div class="alert alert-error m-3 block max-h-full overflow-auto text-xs">
				<p class="font-semibold">The movie could not be played</p>
				<p class="mt-1 font-mono break-words">{error}</p>
				<button class="btn btn-ghost btn-xs mt-1" onclick={() => retry++}>Retry</button>
			</div>
		{/if}
	</div>
</div>
