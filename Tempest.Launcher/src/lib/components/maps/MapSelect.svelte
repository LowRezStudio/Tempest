<script lang="ts">
	//OB57 map list
	//TODO add all maps and version information
	import maps from "$lib/data/maps.json";
	interface PaladinsMap {
		id: string;
		displayName: string;
		iconPath: string;
		mode: string;
	}

	interface Props {
		onselect?: (map: PaladinsMap) => void;
		selectMode?: "vote" | "select";
		votes?: Record<string, string>; //playerId --> mapId
	}

	let { onselect, selectMode, votes }: Props = $props();

	let selectedMap = $state<PaladinsMap | null>(null);
	let hoveredMap = $state<PaladinsMap | null>(null);

	function handleMapClick(map: PaladinsMap) {
		selectedMap = map;
		onselect?.(map);
	}

	function handleMapHover(map: PaladinsMap | null) {
		hoveredMap = map;
	}
</script>

<div class="relative h-full w-full overflow-hidden bg-base-200">
	<!-- Fullscreen Background -->
	<div class="absolute inset-0">
		<video
			src="/champions/empty.webm"
			class="h-full w-full object-cover blur-xs"
			loop
			muted
			playsinline
			autoplay
		></video>
	</div>
	<!-- Content Layer -->
	<div class="relative z-10 flex h-full flex-col">
		<!-- Header -->
		<div class="p-8 pb-2 text-center">
			<h1
				class="text-4xl font-bold text-white"
				style="text-shadow: 0 4px 12px rgba(0,0,0,0.8), 0 2px 4px rgba(0,0,0,0.9);"
			>
				{selectMode == "vote" ? "Vote for the map" : "Select the map"}
			</h1>
		</div>

		<div class="relative flex flex-1 flex-col items-center">
			<div
				class="max-h-[80vh] relative z-10 grid max-w-7xl grid-cols-3 overflow-y-auto p-4 pb-1 scrollbar-hide md:grid-cols-3 lg:grid-cols-4"
			>
				{#each maps as map (map.id)}
					<button
						type="button"
						class={[
							"overflow-hidden border-2 p-0 transition-all duration-200",
							"hover:scale-110 hover:shadow-xl",
							"m-2 lg:m-4 relative",
							selectedMap?.id === map.id ?
								"border-lime-200 ring-3 ring-lime-200 shadow-xl"
							: hoveredMap?.id === map.id ? "border-white/50 ring-3 ring-white/50"
							: "border-base-300",
						]}
						onclick={() => handleMapClick(map)}
						onmouseenter={() => handleMapHover(map)}
						onmouseleave={() => handleMapHover(null)}
					>
						<img
							src={map.iconPath}
							alt={map.displayName}
							class="h-full w-full object-cover"
							loading="lazy"
						/>
						<p
							class="absolute top-0 bg-gradient-to-r from-black/90 via-black/60 to-black/0 w-full text-left"
						>
							{map.displayName}
						</p>
						{#if selectMode == "vote"}
							<p
								class="absolute right-3 bottom-2 rounded-full bg-black w-7 h-7 flex justify-center items-center"
							>
								{votes ? Object.values(votes).filter((m) => m == map.id).length : 0}
							</p>
						{/if}
					</button>
				{/each}
			</div>
		</div>

		{#if selectMode == "select"}
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
					class:btn-accent={selectedMap}
					class:btn-disabled={!selectedMap}
					disabled={!selectedMap}
				>
					Confirm Selection
				</button>
			</div>
		{/if}
	</div>
</div>
