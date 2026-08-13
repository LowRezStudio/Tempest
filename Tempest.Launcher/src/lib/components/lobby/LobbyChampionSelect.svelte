<script lang="ts">
	import champions from "$lib/data/champions.json";
	import { lobbyStaticInfo } from "$lib/lobby/stores.svelte";
	import { m } from "$lib/paraglide/messages";
	import { resolveGamemodeLabel, type Map } from "$lib/types/lobby";
	import ChampionSelect from "../champions/ChampionSelect.svelte";
	import LobbyPlayerCard from "./LobbyPlayerCard.svelte";
	import type { LobbyPlayer } from "$lib/rpc/lobby/lobby_player";

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

	let gamemodeName = $derived(resolveGamemodeLabel(lobbyStaticInfo.value?.gamemode));
</script>

<div class="relative h-full w-full overflow-hidden">
	<!-- Left Side Panel (Blue Gradient) -->
	<div
		class="absolute top-0 bottom-0 left-0 z-20 flex w-48 flex-col gap-3 bg-gradient-to-r from-blue-950/95 via-blue-900/40 to-transparent p-4 pt-16 md:w-64 md:p-6 lg:w-80"
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
		class="absolute top-0 right-0 bottom-0 z-20 flex w-48 flex-col gap-3 bg-gradient-to-l from-red-950/95 via-red-900/40 to-transparent p-4 pt-16 md:w-64 md:p-6 lg:w-80"
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
		<div class="absolute right-8 bottom-8 z-20 w-48 text-right">
			<p class="mb-1 text-sm opacity-70">{gamemodeName}</p>
			<p class="font-semibold text-white">{currentMap.displayName}</p>
			<div class="relative mt-2 h-24 w-full">
				<!-- Clipped Background Container -->
				<div
					class="absolute inset-0 overflow-hidden border border-red-500/30"
					style="clip-path: polygon(24px 0%, 100% 0%, 100% 100%, 0% 100%);"
				>
					<img
						src={currentMap.iconPath}
						alt={currentMap.displayName}
						class="h-full w-full rounded-none object-cover"
					/>
				</div>
				<!-- Slanted Edge Border matching the right team's nameplates exactly, with horizontal Y-axis caps -->
				<svg
					class="absolute top-0 bottom-0 left-0 z-20 h-full w-6 text-red-500"
					viewBox="0 0 24 100"
					preserveAspectRatio="none"
				>
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
