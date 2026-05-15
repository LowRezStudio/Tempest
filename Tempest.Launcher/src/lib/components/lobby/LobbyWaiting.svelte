<script lang="ts">
	import { Users } from "@lucide/svelte";
	import champions from "$lib/data/champions.json";
	import { lobbyWaitingState, teamLeft, teamRight } from "$lib/lobby/stores";
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
	function getChampionDisplayName(champion?: string): string {
		return champions.find((c) => c.name === champion)?.displayName || "";
	}
	function getTitle() {
		const s = $lobbyWaitingState;
		if (s.isWaiting && s.countdownSeconds > 0) return `Starting ${s.countdownSeconds}s`;
		if (s.isWaiting) return m.lobby_title();
		if (s.isLobbyRestarting) return `Restarting ${s.countdownSeconds}`;
		return m.lobby_game_in_progress();
	}
</script>

<Header title={getTitle()}>
	{#snippet icon()}
		<Users size={32} class="opacity-60" />
	{/snippet}
	{#snippet actions()}
		{#if $lobbyWaitingState.canRejoinGame}
			<button class="btn btn-accent" onclick={handleRejoinGame}
				>{m.lobby_rejoin_game()}</button
			>
		{/if}
		{#if !$lobbyWaitingState.canRejoinLobby}
			<button class="btn btn-error" onclick={handleLeave}> {m.lobby_leave_lobby()} </button>
		{:else}
			<button class="btn btn-accent" onclick={handleJoin}> Rejoin Lobby</button>
		{/if}
	{/snippet}
	{#snippet subtitle()}
		{#if $lobbyWaitingState.isWaiting}
			<span>
				{m.lobby_waiting_for_players({
					current: $lobbyWaitingState.playerCount,
					min: $lobbyWaitingState.minimumPlayerCount ?? 0,
				})}
			</span>
		{:else if $lobbyWaitingState.isPendingConnection}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				{m.lobby_connecting()}
			</span>
		{:else if $lobbyWaitingState.isGameServerLaunching}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				{m.lobby_waiting_for_server()}
			</span>
		{:else if $lobbyWaitingState.isLobbyRestarting}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				{m.lobby_lobby_restarting()}
			</span>
		{:else}
			<span>{$lobbyWaitingState.playerCount} {m.lobby_players()}</span>
		{/if}
	{/snippet}
</Header>

<div class="flex-1 flex flex-col overflow-hidden bg-base-100 relative">
	{#if !$lobbyWaitingState.canJoinInProgress || $lobbyWaitingState.canRejoinLobby}
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
		<div class="relative justify-center gap-20 z-10 flex h-full items-center p-8">
			<div class="flex flex-col gap-2">
				{#each $teamLeft as player (player.id)}
					<LobbyPlayerCard
						displayName={player.displayName}
						championIconFolderName={getChampionDisplayName(player.champion)}
						status={getChampionDisplayName(player.champion) || "Not ready"}
					/>
				{/each}
			</div>

			<div class="flex flex-col gap-2">
				{#each $teamRight as player (player.id)}
					<LobbyPlayerCard
						displayName={player.displayName}
						championIconFolderName={getChampionDisplayName(player.champion)}
						status={getChampionDisplayName(player.champion) || "Not ready"}
					/>
				{/each}
			</div>
		</div>
	{:else}
		<LobbyChampionSelect
			teamLeft={$teamLeft}
			teamRight={$teamRight}
			currentMap={$lobbyWaitingState.currentMap}
			handleChampionSelect={handleJoinInProgress}
			gameVersion={$lobbyWaitingState.gameVersion}
			countdownSeconds={-1}
		/>
	{/if}
</div>
