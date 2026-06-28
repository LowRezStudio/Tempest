<script lang="ts">
	import { LogOut, Users } from "@lucide/svelte";
	import champions from "$lib/data/champions.json";
	import { m } from "$lib/paraglide/messages";
	import { teamLeft, teamRight } from "$lib/lobby/stores.svelte";
	import MapSelect from "../maps/MapSelect.svelte";
	import Header from "../ui/Header.svelte";
	import LobbyPlayerCard from "./LobbyPlayerCard.svelte";

	interface Props {
		handleLeave: () => void;
		playerCount: number;
		handleMapSelect: (id: string) => void;
		votes?: Record<string, string>;
		gameVersion: string;
		gamemode: string;
		countdownSeconds: number;
	}

	let {
		handleLeave,
		playerCount,
		handleMapSelect,
		votes,
		gameVersion,
		gamemode,
		countdownSeconds,
	}: Props = $props();

	function getChampionDisplayName(champion: string | undefined): string {
		return champions.find((c) => c.name === champion)?.displayName || "";
	}
</script>

<div class="relative h-full w-full">
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

	<div class="relative z-10 flex flex-col h-full">
		<Header
			title={countdownSeconds > 0 ?
				`${m.lobby_map_vote()} ${countdownSeconds}`
			:	m.lobby_map_vote()}
			class="bg-base-200/90 backdrop-blur-xs"
		>
			{#snippet icon()}
				<Users size={32} class="opacity-60" />
			{/snippet}
			{#snippet actions()}
				<button class="btn btn-error" onclick={handleLeave}>
					<LogOut size={18} />
					{m.lobby_leave_lobby()}
				</button>
			{/snippet}
			{#snippet subtitle()}
				<span>{playerCount} {m.lobby_players({ count: playerCount })}</span>
			{/snippet}
		</Header>

		<div class="min-h-0 mx-48 md:mx-64 lg:mx-80">
			<MapSelect
				onselect={(map) => handleMapSelect(map.id)}
				selectMode="vote"
				{votes}
				{gameVersion}
				{gamemode}
			/>
		</div>
	</div>

	<!-- Left Side Panel (Blue Gradient) -->
	<div
		class="absolute top-0 left-0 bottom-0 w-48 md:w-64 lg:w-80 z-20 bg-gradient-to-r from-blue-950/95 via-blue-900/40 to-transparent flex flex-col p-4 md:p-6 pt-16 gap-3"
	>
		{#each teamLeft.value as player (player.id)}
			<LobbyPlayerCard
				displayName={player.displayName}
				championIconFolderName={getChampionDisplayName(player.champion)}
				status={player.champion ? getChampionDisplayName(player.champion) : m.lobby_not_ready()}
				team="left"
				compact={true}
			/>
		{/each}
	</div>

	<!-- Right Side Panel (Red Gradient) -->
	<div
		class="absolute top-0 right-0 bottom-0 w-48 md:w-64 lg:w-80 z-20 bg-gradient-to-l from-red-950/95 via-red-900/40 to-transparent flex flex-col p-4 md:p-6 pt-16 gap-3"
	>
		{#each teamRight.value as player (player.id)}
			<LobbyPlayerCard
				displayName={player.displayName}
				championIconFolderName={getChampionDisplayName(player.champion)}
				status={player.champion ? getChampionDisplayName(player.champion) : m.lobby_not_ready()}
				team="right"
				compact={true}
			/>
		{/each}
	</div>
</div>
