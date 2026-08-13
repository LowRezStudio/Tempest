<script lang="ts">
	import allChampions from "$lib/data/champions.json";
	import { m } from "$lib/paraglide/messages";
	import { compareVersions } from "$lib/utils/versions";

	interface Champion {
		name: string;
		displayName: string;
		iconPath: string;
		fallbackPath: string;
		videoPath: string;
	}

	interface Props {
		onselect?: (champion: Champion) => void;
		confirmedChampionName?: string;
		gameVersion?: string;
		countdownSeconds?: number;
		sidebarPadding?: boolean;
	}

	let {
		onselect,
		confirmedChampionName,
		// TODO: Remove 0.57 placeholder
		gameVersion = "0.57",
		countdownSeconds = -1,
		sidebarPadding = false,
	}: Props = $props();

	const champions = allChampions
		.filter((champ) => {
			return compareVersions(gameVersion, champ.version) >= 0;
		})
		.sort((a, b) => a.displayName.localeCompare(b.displayName))
		.map((champ) => ({
			name: champ.name,
			displayName: champ.displayName,
			iconPath: `/champions/${champ.displayName}/icon.webp`,
			fallbackPath: `/champions/${champ.displayName}/fallback.webp`,
			videoPath: `/champions/${champ.displayName}/video.webm`,
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
	}
	function handleConfirm() {
		if (onselect && selectedChampion) {
			onselect?.(selectedChampion);
		}
	}

	function handleChampionHover(champion: Champion | null) {
		hoveredChampion = champion;
	}

	// Get the champion to display
	let displayedChampion = $derived(
		champions.find((c) => c.name === confirmedChampionName) || selectedChampion,
	);

	// Load video when background champion changes
	$effect(() => {
		if (videoElement && displayedChampion) {
			// Force dimensions before loading (WebKit fix)
			videoElement.style.width = "100%";
			videoElement.style.height = "100%";

			videoElement.src = displayedChampion.videoPath;
			videoElement.poster = displayedChampion.fallbackPath;
			videoElement.load();

			// Wait for metadata to be loaded before playing (WebKit fix)
			const handleMetadata = () => {
				// Start playing
				videoElement?.play().catch(() => {
					// Ignore autoplay errors
				});
			};

			// Fallback to empty video if loading fails
			const handleError = () => {
				if (videoElement) {
					const currentSrc = videoElement.src;
					if (!currentSrc.endsWith("/champions/empty.webm")) {
						videoElement.src = "/champions/empty.webm";
						videoElement.load();
						videoElement.play().catch(() => {});
					}
				}
			};

			videoElement.addEventListener("loadedmetadata", handleMetadata, { once: true });
			videoElement.addEventListener("error", handleError, { once: true });

			// Update previous champion after current one starts loading
			previousChampion = displayedChampion;

			return () => {
				videoElement?.removeEventListener("loadedmetadata", handleMetadata);
				videoElement?.removeEventListener("error", handleError);
			};
		}
	});
</script>

<div class="bg-base-200 relative h-full w-full overflow-hidden">
	<!-- Fullscreen Background -->
	<div class="absolute inset-0">
		{#if displayedChampion}
			<!-- Previous champion fallback (to prevent flash) -->
			{#if previousChampion && previousChampion.name !== displayedChampion.name}
				<img
					src={previousChampion.fallbackPath}
					alt={previousChampion.displayName}
					class="absolute inset-0 h-full w-full object-cover object-[75%_center]"
				/>
			{/if}

			<!-- Current champion fallback (always visible below video) -->
			<img
				src={displayedChampion.fallbackPath}
				alt={displayedChampion.displayName}
				class="absolute inset-0 h-full w-full object-cover object-[75%_center]"
			/>

			<!-- Video Layer -->
			<video
				bind:this={videoElement}
				poster={displayedChampion.fallbackPath}
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

	<!-- Bottom gradient blur (Full screen width) -->
	{#if !confirmedChampionName}
		<div class="pointer-events-none absolute right-0 bottom-0 left-0 z-10 h-64">
			<div
				class="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent"
			></div>
			<div
				class="absolute inset-0"
				style="backdrop-filter: blur(12px); -webkit-backdrop-filter: blur(12px); mask-image: linear-gradient(to top, black, transparent); -webkit-mask-image: linear-gradient(to top, black, transparent);"
			></div>
		</div>
	{/if}

	<!-- Content Layer -->
	<div
		class="relative z-20 flex h-full flex-col {sidebarPadding ? 'mx-48 md:mx-64 lg:mx-80' : ''}"
		style="container-type: inline-size;"
	>
		<!-- Counter -->
		{#if countdownSeconds > 0}
			<div class="flex justify-center pt-4">
				<h2
					class="text-6xl font-bold text-white"
					style="text-shadow: 0 4px 12px rgba(0,0,0,0.8), 0 2px 4px rgba(0,0,0,0.9);"
				>
					{countdownSeconds}
				</h2>
			</div>
		{/if}

		<!-- Floating Champion Portraits -->
		{#if !confirmedChampionName}
			<div class="flex w-full flex-1 flex-col items-center justify-center gap-6 pt-16 pb-8">
				<div class="relative flex w-full max-w-6xl justify-center">
					<div
						bind:this={scrollContainer}
						class="scrollbar-hide champ-grid relative z-10 grid max-h-[480px] w-full max-w-6xl justify-center gap-2 overflow-y-auto p-4 md:gap-3"
					>
						{#each champions as champion (champion.name)}
							<button
								type="button"
								class={[
									"champ-btn cursor-pointer overflow-hidden rounded-none border-2 p-0 transition-all duration-200",
									"hover:scale-110 hover:shadow-xl",
									displayedChampion?.name === champion.name
										? "border-accent ring-accent shadow-xl ring-3"
										: hoveredChampion?.name === champion.name
											? "border-white/50 ring-3 ring-white/50"
											: "border-base-300",
								]}
								class:pointer-events-none={confirmedChampionName}
								onclick={() => handleChampionClick(champion)}
								onmouseenter={() => handleChampionHover(champion)}
								onmouseleave={() => handleChampionHover(null)}
							>
								<img
									src={champion.iconPath}
									alt={champion.name}
									class="h-full w-full object-cover object-center"
									loading="lazy"
									onerror={(e) => {
										(e.currentTarget as HTMLImageElement).src =
											"/champions/Generic/icon.webp";
									}}
								/>
							</button>
						{/each}
					</div>

					<!-- Scroll indicator -->
					{#if canScrollDown}
						<div
							class="pointer-events-none absolute right-0 bottom-0 left-0 z-20 flex justify-center"
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
				<div class="relative flex w-full flex-col items-center gap-3 text-center">
					<button
						type="button"
						class="btn btn-xl btn-wide relative z-10 cursor-pointer font-bold uppercase shadow-none"
						class:btn-accent={displayedChampion && !confirmedChampionName}
						class:btn-disabled={!displayedChampion || !!confirmedChampionName}
						disabled={!displayedChampion || !!confirmedChampionName}
						onclick={handleConfirm}
					>
						{m.common_confirm_selection()}
					</button>

					<h2
						class="pointer-events-none mt-4 text-4xl font-bold text-white uppercase select-none"
						class:invisible={!displayedChampion}
						style="text-shadow: 0 4px 12px rgba(0,0,0,0.8), 0 2px 4px rgba(0,0,0,0.9);"
					>
						{displayedChampion?.displayName ?? "Select Champion"}
					</h2>
				</div>
			</div>
		{:else}
			<div class="flex flex-1 flex-col items-center justify-end pb-8">
				{#if displayedChampion}
					<h2
						class="pointer-events-none text-4xl font-bold text-white uppercase select-none"
						style="text-shadow: 0 4px 12px rgba(0,0,0,0.8), 0 2px 4px rgba(0,0,0,0.9);"
					>
						{displayedChampion.displayName}
					</h2>
				{/if}
			</div>
		{/if}
	</div>
</div>

<style>
	.champ-grid {
		grid-template-columns: repeat(auto-fill, 48px);
	}
	.champ-btn {
		width: 48px;
		height: 64px;
	}
	@container (min-width: 520px) {
		.champ-grid {
			grid-template-columns: repeat(auto-fill, 64px);
		}
		.champ-btn {
			width: 64px;
			height: 80px;
		}
	}
</style>
