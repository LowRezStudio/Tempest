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
	let scrollContainer = $state<HTMLDivElement | null>(null);
	let hasOverflow = $state(false);
	let canScrollDown = $state(false);
	let previousChampion = $state<Champion | null>(null);

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
			// Force dimensions before loading (WebKit fix)
			videoElement.style.width = "100%";
			videoElement.style.height = "100%";

			videoElement.src = backgroundChampion.videoPath;
			videoElement.poster = backgroundChampion.fallbackPath;
			videoElement.load();

			// Wait for metadata to be loaded before playing (WebKit fix)
			const handleMetadata = () => {
				// Start playing
				videoElement?.play().catch(() => {
					// Ignore autoplay errors
				});
			};

			videoElement.addEventListener("loadedmetadata", handleMetadata, { once: true });

			// Update previous champion after current one starts loading
			previousChampion = backgroundChampion;
		}
	});
</script>

<div class="relative h-full w-full overflow-hidden bg-base-200">
	<!-- Fullscreen Background -->
	<div class="absolute inset-0">
		{#if backgroundChampion}
			<!-- Previous champion fallback (to prevent flash) -->
			{#if previousChampion && previousChampion.name !== backgroundChampion.name}
				<img
					src={previousChampion.fallbackPath}
					alt={previousChampion.name}
					class="absolute inset-0 h-full w-full object-cover object-[75%_center]"
				/>
			{/if}

			<!-- Current champion fallback (always visible below video) -->
			<img
				src={backgroundChampion.fallbackPath}
				alt={backgroundChampion.name}
				class="absolute inset-0 h-full w-full object-cover object-[75%_center]"
			/>

			<!-- Video Layer -->
			<video
				bind:this={videoElement}
				poster={backgroundChampion.fallbackPath}
				class="absolute inset-0 !h-full !w-full object-cover object-[75%_center]"
				loop
				muted
				playsinline
				preload="metadata"
			></video>
		{:else}
			<!-- Default empty background video with blur -->
			<video
				src="/champions/empty.webm"
				class="h-full w-full object-cover blur-xs"
				loop
				muted
				playsinline
				autoplay
			></video>
		{/if}
	</div>

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
				class="relative z-10 grid max-h-[304px] max-w-6xl grid-cols-6 gap-3 overflow-y-auto p-4 scrollbar-hide md:grid-cols-8 lg:grid-cols-11"
			>
				{#each champions as champion (champion.name)}
					<button
						type="button"
						class={[
							"h-18 w-18 overflow-hidden rounded-full border-2 p-0 transition-all duration-200",
							"hover:scale-110 hover:shadow-xl",
							selectedChampion?.name === champion.name ?
								"border-accent ring-3 ring-accent shadow-xl"
							: hoveredChampion?.name === champion.name ?
								"border-white/50 ring-3 ring-white/50"
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
					class="pointer-events-none absolute bottom-0 z-20 flex justify-center"
					style="text-shadow: 0 2px 8px rgba(0,0,0,0.8);"
				>
					<svg
						class="h-8 w-8 animate-bounce text-white drop-shadow-lg"
						fill="none"
						stroke="currentColor"
						viewBox="0 0 24 24"
						style="filter: drop-shadow(0 0 8px rgba(255,255,255,0.5));"
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
		<div class="relative w-full pb-8 pt-4 text-center">
			<!-- Bottom gradient blur -->
			<div class="pointer-events-none absolute bottom-0 left-0 right-0 h-64">
				<div
					class="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent"
				></div>
				<div
					class="absolute inset-0"
					style="backdrop-filter: blur(12px); -webkit-backdrop-filter: blur(12px); mask-image: linear-gradient(to top, black, transparent); -webkit-mask-image: linear-gradient(to top, black, transparent);"
				></div>
			</div>

			<button
				type="button"
				class="btn btn-lg relative z-10 shadow-xl"
				class:btn-accent={selectedChampion}
				class:btn-disabled={!selectedChampion}
				disabled={!selectedChampion}
			>
				Confirm Selection
			</button>
		</div>
	</div>
</div>
