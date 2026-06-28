<script lang="ts">
	import { Users } from "@lucide/svelte";
	import champions from "$lib/data/champions.json";
	import { lobbyWaitingState, teamLeft, teamRight } from "$lib/lobby/stores.svelte";
	import { m } from "$lib/paraglide/messages";
	import ChampionSelect from "../champions/ChampionSelect.svelte";
	import Header from "../ui/Header.svelte";
	import LobbyChampionSelect from "./LobbyChampionSelect.svelte";
	import LobbyPlayerCard from "./LobbyPlayerCard.svelte";

	interface Props {
		handleRejoinGame: () => void;
		handleRejoinLobby: () => void;
		handleJoinInProgress: (champ: string) => void;
		handleLeave: () => void;
	}

	let {
		handleRejoinGame,
		handleRejoinLobby: handleJoin,
		handleJoinInProgress,
		handleLeave,
	}: Props = $props();
	function getChampionDisplayName(champion: string | undefined): string {
		return champions.find((c) => c.name === champion)?.displayName || "";
	}
	function getTitle() {
		const s = lobbyWaitingState.value;
		if (s.isWaiting && s.countdownSeconds > 0) {
			return m.lobby_starting_countdown({ seconds: s.countdownSeconds });
		}
		if (s.isWaiting) return m.lobby_title();
		if (s.isLobbyRestarting) {
			return m.lobby_restarting_countdown({ seconds: s.countdownSeconds });
		}
		return m.lobby_game_in_progress();
	}
</script>

<Header title={getTitle()}>
	{#snippet icon()}
		<Users size={32} class="opacity-60" />
	{/snippet}
	{#snippet actions()}
		{#if lobbyWaitingState.value.canRejoinGame}
			<button class="btn btn-accent" onclick={handleRejoinGame}
				>{m.lobby_rejoin_game()}</button
			>
		{/if}
		{#if !lobbyWaitingState.value.canRejoinLobby}
			<button class="btn btn-error" onclick={handleLeave}> {m.lobby_leave_lobby()} </button>
		{:else}
			<button class="btn btn-accent" onclick={handleJoin}> {m.lobby_rejoin_lobby()}</button>
		{/if}
	{/snippet}
	{#snippet subtitle()}
		{#if lobbyWaitingState.value.isWaiting}
			<span>
				{m.lobby_waiting_for_players({
					current: lobbyWaitingState.value.playerCount,
					min: lobbyWaitingState.value.minimumPlayerCount ?? 0,
				})}
			</span>
		{:else if lobbyWaitingState.value.isPendingConnection}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				{m.lobby_connecting()}
			</span>
		{:else if lobbyWaitingState.value.isGameServerLaunching}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				{m.lobby_waiting_for_server()}
			</span>
		{:else if lobbyWaitingState.value.isLobbyRestarting}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				{m.lobby_lobby_restarting()}
			</span>
		{:else}
			<span
				>{lobbyWaitingState.value.playerCount}
				{m.lobby_players({ count: lobbyWaitingState.value.playerCount })}</span
			>
		{/if}
	{/snippet}
</Header>

<div class="flex-1 flex flex-col overflow-hidden bg-base-100 relative">
	{#if !lobbyWaitingState.value.canJoinInProgress || lobbyWaitingState.value.canRejoinLobby}
		<div class="absolute inset-0">
			{#if lobbyWaitingState.value.currentMap}
				<img
					src={lobbyWaitingState.value.currentMap.iconPath}
					alt={lobbyWaitingState.value.currentMap.displayName}
					class="h-full w-full object-cover blur-xs"
				/>
			{:else}
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
		<!-- Left Side Panel (Blue Gradient) -->
		<div
			class="absolute top-0 left-0 bottom-0 w-48 md:w-64 lg:w-80 z-20 bg-gradient-to-r from-blue-950/95 via-blue-900/40 to-transparent flex flex-col p-4 md:p-6 pt-16 gap-3"
		>
			{#each teamLeft.value as player (player.id)}
				<LobbyPlayerCard
					displayName={player.displayName}
					championIconFolderName={getChampionDisplayName(player.champion)}
					status={getChampionDisplayName(player.champion) || m.lobby_not_ready()}
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
					status={getChampionDisplayName(player.champion) || m.lobby_not_ready()}
					team="right"
					compact={true}
				/>
			{/each}
		</div>
	{:else}
		<LobbyChampionSelect
			teamLeft={teamLeft.value}
			teamRight={teamRight.value}
			currentMap={lobbyWaitingState.value.currentMap}
			handleChampionSelect={handleJoinInProgress}
			gameVersion={lobbyWaitingState.value.gameVersion}
			countdownSeconds={-1}
		/>
	{/if}
</div>
