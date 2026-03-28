<script lang="ts">
	import PlayerCard from "$lib/components/lobby/LobbyPlayerCard.svelte";
	import champions from "$lib/data/champions.json";
	import ChampionSelect from "../champions/ChampionSelect.svelte";
	import type { LobbyPlayer } from "$lib/rpc/lobby/lobby_player";
	import type { Map } from "$lib/types/lobby";

	interface Props {
		teamLeft: readonly LobbyPlayer[];
		teamRight: readonly LobbyPlayer[];
		currentMap?: Map;
		confirmedChampion?: string;
		handleChampionSelect: (champ: string) => void;
		gameVersion: string;
	}

	let {
		teamLeft,
		teamRight,
		currentMap,
		confirmedChampion,
		handleChampionSelect,
		gameVersion,
	}: Props = $props();

	function getChampionDisplayName(champion?: string): string {
		return champions.find((c) => c.name === champion)?.displayName || "";
	}
</script>

<div class="relative h-full w-full">
	<div class="absolute inset-0 z-10">
		<div class="absolute top-0 left-0 p-4 z-20">
			<div class="flex flex-col gap-2">
				{#each teamLeft as player (player.id)}
					<PlayerCard
						displayName={player.displayName}
						championIconFolderName={getChampionDisplayName(player.champion)}
						status={getChampionDisplayName(player.champion) || "Not ready"}
					/>
				{/each}
			</div>
		</div>

		<div class="absolute top-0 right-0 p-4 z-20">
			<div class="flex flex-col gap-2">
				{#each teamRight as player (player.id)}
					<PlayerCard
						displayName={player.displayName}
						championIconFolderName={getChampionDisplayName(player.champion)}
						status={getChampionDisplayName(player.champion) || "Not ready"}
					/>
				{/each}
			</div>
		</div>

		{#if currentMap}
			<div class="absolute top-0 right-0 p-4 z-20 mt-36">
				<div class="bg-base-200/90 backdrop-blur-xs rounded-lg p-3 w-48">
					<p class="text-sm opacity-70 mb-1">Playing on</p>
					<p class="font-semibold">{currentMap.displayName}</p>
					<img
						src={currentMap.iconPath}
						alt={currentMap.displayName}
						class="w-full h-24 object-cover rounded mt-2"
					/>
				</div>
			</div>
		{/if}
	</div>
	<ChampionSelect
		confirmedChampionName={confirmedChampion}
		onselect={(champ) => handleChampionSelect(champ.name)}
		{gameVersion}
	/>
</div>
