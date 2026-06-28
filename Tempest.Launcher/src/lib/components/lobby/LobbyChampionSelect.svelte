<script lang="ts">
	import champions from "$lib/data/champions.json";
	import { lobbyStaticInfo } from "$lib/lobby/stores.svelte";
	import { m } from "$lib/paraglide/messages";
	import ChampionSelect from "../champions/ChampionSelect.svelte";
	import type { LobbyPlayer } from "$lib/rpc/lobby/lobby_player";
	import { resolveGamemodeLabel, type Map } from "$lib/types/lobby";
	import LobbyPlayerCard from "./LobbyPlayerCard.svelte";

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
			<LobbyPlayerCard
				displayName={player.displayName}
				championIconFolderName={champDisplayName}
				status={player.champion ? champDisplayName : m.lobby_not_ready()}
				team="left"
				compact={true}
			/>
		{/each}
	</div>

	<!-- Right Side Panel (Red Gradient) -->
	<div
		class="absolute top-0 right-0 bottom-0 w-48 md:w-64 lg:w-80 z-20 bg-gradient-to-l from-red-950/95 via-red-900/40 to-transparent flex flex-col p-4 md:p-6 pt-16 gap-3"
	>
		{#each teamRight as player (player.id)}
			{@const champDisplayName = getChampionDisplayName(player.champion)}
			<LobbyPlayerCard
				displayName={player.displayName}
				championIconFolderName={champDisplayName}
				status={player.champion ? champDisplayName : m.lobby_not_ready()}
				team="right"
				compact={true}
			/>
		{/each}
	</div>

	<!-- Floating Map Card -->
	{#if currentMap}
		<div class="absolute bottom-8 right-8 z-20 text-right w-48">
			<p class="text-sm opacity-70 mb-1">{gamemodeName}</p>
			<p class="font-semibold text-white">{currentMap.displayName}</p>
			<div class="relative w-full h-24 mt-2">
				<!-- Clipped Background Container -->
				<div
					class="absolute inset-0 overflow-hidden border border-red-500/30"
					style="clip-path: polygon(24px 0%, 100% 0%, 100% 100%, 0% 100%);"
				>
					<img
						src={currentMap.iconPath}
						alt={currentMap.displayName}
						class="w-full h-full object-cover rounded-none"
					/>
				</div>
				<!-- Slanted Edge Border matching the right team's nameplates exactly, with horizontal Y-axis caps -->
				<svg class="absolute top-0 left-0 bottom-0 h-full w-6 text-red-500 z-20" viewBox="0 0 24 100" preserveAspectRatio="none">
					<polygon points="16,0 24,0 8,100 0,100" fill="currentColor" />
				</svg>
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
