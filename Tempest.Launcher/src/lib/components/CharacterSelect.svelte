<script lang="ts">
	interface Champion {
		name: string;
		iconPath: string;
		fallbackPath: string;
		videoPath: string;
	}

	interface Props {
		onselect?: (champion: Champion) => void;
	}

	let { onselect }: Props = $props();

	const champions: Champion[] = [
		"Androxus",
		"Ash",
		"Barik",
		"Bomb King",
		"Buck",
		"Cassie",
		"Drogoz",
		"Evie",
		"Fernando",
		"Grohk",
		"Grover",
		"Inara",
		"Jenos",
		"Kinessa",
		"Lex",
		"Lian",
		"Maeve",
		"Makoa",
		"Mal'Damba",
		"Pip",
		"Ruckus",
		"Seris",
		"Sha Lin",
		"Skye",
		"Strix",
		"Talus",
		"Terminus",
		"Torvald",
		"Tyra",
		"Viktor",
		"Vivian",
		"Willo",
		"Zhin",
	].map((name) => ({
		name,
		iconPath: `/champions/${name}/icon.webp`,
		fallbackPath: `/champions/${name}/fallback.webp`,
		videoPath: `/champions/${name}/video.webm`,
	}));

	let selectedChampion = $state<Champion | null>(null);
	let hoveredChampion = $state<Champion | null>(null);
	let videoElement = $state<HTMLVideoElement | null>(null);
	let isVideoLoaded = $state(false);
	let scrollContainer = $state<HTMLDivElement | null>(null);
	let hasOverflow = $state(false);
	let canScrollDown = $state(false);

	// Check if content overflows
	$effect(() => {
		if (scrollContainer) {
			const container = scrollContainer;
			const checkOverflow = () => {
				const overflow = container.scrollHeight > container.clientHeight;
				hasOverflow = overflow;
				canScrollDown =
					overflow &&
					container.scrollTop < container.scrollHeight - container.clientHeight - 5;
			};

			checkOverflow();
			container.addEventListener("scroll", checkOverflow);

			return () => container.removeEventListener("scroll", checkOverflow);
		}
	});

	function handleChampionClick(champion: Champion) {
		selectedChampion = champion;
		onselect?.(champion);
	}

	function handleChampionHover(champion: Champion | null) {
		hoveredChampion = champion;
	}

	// Get the champion to display in the background (only on selection, not hover)
	let backgroundChampion = $derived(selectedChampion);

	// Load video when background champion changes
	$effect(() => {
		if (videoElement && backgroundChampion) {
			// Hide video immediately when champion changes
			isVideoLoaded = false;

			// Force dimensions before loading (WebKit fix)
			videoElement.style.width = "100%";
			videoElement.style.height = "100%";

			videoElement.src = backgroundChampion.videoPath;
			videoElement.poster = backgroundChampion.fallbackPath;
			videoElement.load();

			// Wait for metadata to be loaded before playing (WebKit fix)
			const handleMetadata = () => {
				// Start playing but keep hidden until first frame is rendered
				videoElement?.play().catch(() => {
					// Ignore autoplay errors
				});
			};

			videoElement.addEventListener("loadedmetadata", handleMetadata, { once: true });
		} else {
			isVideoLoaded = false;
		}
	});

	function handleVideoLoaded() {
		// Wait for video to have proper dimensions and first frame rendered (WebKit fix)
		if (videoElement && videoElement.videoWidth > 0 && videoElement.videoHeight > 0) {
			setTimeout(() => {
				isVideoLoaded = true;
			}, 150);
		}
	}
</script>

<div class="relative h-full w-full overflow-hidden bg-base-200">
	<!-- Fullscreen Background -->
	{#if backgroundChampion}
		<div class="absolute inset-0">
			<!-- Fallback Image (always visible) -->
			<img
				src={backgroundChampion.fallbackPath}
				alt={backgroundChampion.name}
				class="h-full w-full object-cover object-[75%_center]"
			/>

			<!-- Video Layer (fades in when loaded) -->
			<video
				bind:this={videoElement}
				poster={backgroundChampion.fallbackPath}
				class="absolute inset-0 !h-full !w-full object-cover object-[75%_center] transition-opacity duration-500"
				class:opacity-0={!isVideoLoaded}
				class:opacity-100={isVideoLoaded}
				loop
				muted
				playsinline
				preload="metadata"
				onloadeddata={handleVideoLoaded}
			></video>
		</div>
	{/if}

	<!-- Content Layer -->
	<div class="relative z-10 flex h-full flex-col">
		<!-- Header -->
		<div class="p-8 text-center">
			<h1
				class="text-4xl font-bold text-white"
				style="text-shadow: 0 4px 12px rgba(0,0,0,0.8), 0 2px 4px rgba(0,0,0,0.9);"
			>
				Select Your Champion
			</h1>
			{#if selectedChampion}
				<h2
					class="mt-4 text-6xl font-bold text-white"
					style="text-shadow: 0 4px 12px rgba(0,0,0,0.8), 0 2px 4px rgba(0,0,0,0.9);"
				>
					{selectedChampion.name}
				</h2>
			{/if}
		</div>

		<!-- Floating Champion Portraits -->
		<div class="relative flex flex-1 flex-col items-center justify-end">
			<div
				bind:this={scrollContainer}
				class="grid max-h-[304px] max-w-6xl grid-cols-6 gap-4 overflow-y-auto p-4 scrollbar-hide md:grid-cols-8 lg:grid-cols-11"
			>
				{#each champions as champion (champion.name)}
					<button
						type="button"
						class={[
							"h-20 w-20 overflow-hidden rounded-full border-2 p-0 transition-all duration-200",
							"hover:scale-110 hover:shadow-xl",
							selectedChampion?.name === champion.name ?
								"border-accent ring-4 ring-accent shadow-xl"
							: hoveredChampion?.name === champion.name ?
								"border-white/50 ring-4 ring-white/50"
							:	"border-base-300",
						]}
						onclick={() => handleChampionClick(champion)}
						onmouseenter={() => handleChampionHover(champion)}
						onmouseleave={() => handleChampionHover(null)}
					>
						<img
							src={champion.iconPath}
							alt={champion.name}
							class="h-full w-full object-cover"
							loading="lazy"
						/>
					</button>
				{/each}
			</div>

			<!-- Scroll indicator -->
			{#if canScrollDown}
				<div
					class="pointer-events-none absolute bottom-0 flex justify-center"
					style="text-shadow: 0 2px 8px rgba(0,0,0,0.8);"
				>
					<svg
						class="h-8 w-8 animate-bounce text-white"
						fill="none"
						stroke="currentColor"
						viewBox="0 0 24 24"
					>
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M19 9l-7 7-7-7"
						></path>
					</svg>
				</div>
			{/if}
		</div>

		<!-- Confirm Button -->
		<div class="w-full pb-8 pt-4 text-center">
			<button
				type="button"
				class="btn btn-lg shadow-xl"
				class:btn-accent={selectedChampion}
				class:btn-disabled={!selectedChampion}
				disabled={!selectedChampion}
			>
				Confirm Selection
			</button>
		</div>
	</div>
</div>
