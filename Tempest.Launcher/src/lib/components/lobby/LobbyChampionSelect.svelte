<script lang="ts">
	import champions from "$lib/data/champions.json";
	import { lobbyStaticInfo } from "$lib/lobby/stores.svelte";
	import { m } from "$lib/paraglide/messages";
	import ChampionSelect from "../champions/ChampionSelect.svelte";
	import type { LobbyPlayer } from "$lib/rpc/lobby/lobby_player";
	import { resolveGamemodeLabel, type Map } from "$lib/types/lobby";

	interface Props {
		teamLeft: readonly LobbyPlayer[];
		teamRight: readonly LobbyPlayer[];
		currentMap?: Map;
		confirmedChampion?: string;
		handleChampionSelect: (champ: string) => void;
		gameVersion: string;
		countdownSeconds: number;
	}

	let {
		teamLeft,
		teamRight,
		currentMap,
		confirmedChampion,
		handleChampionSelect,
		gameVersion,
		countdownSeconds,
	}: Props = $props();

	function getChampionDisplayName(champion: string | undefined): string {
		return champions.find((c) => c.name === champion)?.displayName || "";
	}

	let gamemodeName = $derived(
		resolveGamemodeLabel(lobbyStaticInfo.value?.gamemode),
	);
</script>

<div class="relative h-full w-full overflow-hidden">
	<!-- Left Side Panel (Blue Gradient) -->
	<div
		class="absolute top-0 left-0 bottom-0 w-48 md:w-64 lg:w-80 z-20 bg-gradient-to-r from-blue-950/95 via-blue-900/40 to-transparent flex flex-col p-4 md:p-6 pt-16 gap-3"
	>
		{#each teamLeft as player (player.id)}
			{@const champDisplayName = getChampionDisplayName(player.champion)}
			<div
				class="flex items-center gap-2.5 bg-base-200/90 backdrop-blur-xs rounded-lg p-1.5 shadow-md"
			>
				<div
					class="relative w-10 h-10 flex-shrink-0 bg-neutral-900 overflow-hidden border border-blue-500/20"
				>
					<img
						src={`/champions/${champDisplayName || "Generic"}/icon.webp`}
						alt={champDisplayName || "No Champion"}
						class="w-full h-full object-cover rounded-none"
						loading="lazy"
						onerror={(e) => {
							(e.currentTarget as HTMLImageElement).src = "/champions/Generic/icon.webp";
						}}
					/>
				</div>
				<div class="min-w-0 flex-1">
					<p
						class="text-sm font-semibold truncate text-white leading-tight"
					>
						{player.displayName}
					</p>
					<p
						class="text-xs opacity-75 truncate leading-none {player.champion
							? 'text-blue-400'
							: 'text-white/50'}"
					>
						{player.champion
							? champDisplayName
							: m.lobby_not_ready()}
					</p>
				</div>
			</div>
		{/each}
	</div>

	<!-- Right Side Panel (Red Gradient) -->
	<div
		class="absolute top-0 right-0 bottom-0 w-48 md:w-64 lg:w-80 z-20 bg-gradient-to-l from-red-950/95 via-red-900/40 to-transparent flex flex-col p-4 md:p-6 pt-16 gap-3"
	>
		{#each teamRight as player (player.id)}
			{@const champDisplayName = getChampionDisplayName(player.champion)}
			<div
				class="flex items-center gap-2.5 bg-base-200/90 backdrop-blur-xs rounded-lg p-1.5 shadow-md"
			>
				<div
					class="relative w-10 h-10 flex-shrink-0 bg-neutral-900 overflow-hidden border border-red-500/20"
				>
					<img
						src={`/champions/${champDisplayName || "Generic"}/icon.webp`}
						alt={champDisplayName || "No Champion"}
						class="w-full h-full object-cover rounded-none"
						loading="lazy"
						onerror={(e) => {
							(e.currentTarget as HTMLImageElement).src = "/champions/Generic/icon.webp";
						}}
					/>
				</div>
				<div class="min-w-0 flex-1">
					<p
						class="text-sm font-semibold truncate text-white leading-tight"
					>
						{player.displayName}
					</p>
					<p
						class="text-xs opacity-75 truncate leading-none {player.champion
							? 'text-red-400'
							: 'text-white/50'}"
					>
						{player.champion
							? champDisplayName
							: m.lobby_not_ready()}
					</p>
				</div>
			</div>
		{/each}
	</div>

	<!-- Floating Map Card -->
	{#if currentMap}
		<div class="absolute bottom-8 right-8 z-20">
			<div
				class="bg-base-200/90 backdrop-blur-xs rounded-lg p-3 w-48 shadow-lg"
			>
				<p class="text-sm opacity-70 mb-1">{gamemodeName}</p>
				<p class="font-semibold">{currentMap.displayName}</p>
				<img
					src={currentMap.iconPath}
					alt={currentMap.displayName}
					class="w-full h-24 object-cover rounded mt-2"
				/>
			</div>
		</div>
	{/if}

	<ChampionSelect
		confirmedChampionName={confirmedChampion}
		onselect={(champ) => handleChampionSelect(champ.name)}
		{countdownSeconds}
		{gameVersion}
		sidebarPadding={true}
	/>
</div>
