<script lang="ts">
	import { Users } from "@lucide/svelte";
	import champions from "$lib/data/champions.json";
	import { m } from "$lib/paraglide/messages";
	import Header from "../ui/Header.svelte";
	import LobbyPlayerCard from "./LobbyPlayerCard.svelte";
	import type { LobbyPlayer } from "$lib/rpc/lobby/lobby_player";

	interface Props {
		isGameInProgress: boolean;
		isWaiting: boolean;
		isPendingConnection: boolean;
		isGameServerLaunching: boolean;
		isLobbyRestarting: boolean;
		isLaunchingClient: boolean;
		canRejoinGame: boolean;
		playerCount: number;
		minimumPlayerCount: number;
		teamLeft: readonly LobbyPlayer[];
		teamRight: readonly LobbyPlayer[];
		countdownSeconds: number;
		handleRejoin: () => void;
		handleLeave: () => void;
	}

	let {
		isGameInProgress,
		isWaiting,
		isPendingConnection,
		isGameServerLaunching,
		isLobbyRestarting,
		isLaunchingClient,
		canRejoinGame,
		playerCount,
		minimumPlayerCount,
		teamLeft,
		teamRight,
		countdownSeconds,
		handleRejoin,
		handleLeave,
	}: Props = $props();
	function getChampionDisplayName(champion?: string): string {
		return champions.find((c) => c.name === champion)?.displayName || "";
	}
	function getTitle() {
		if (isWaiting && countdownSeconds > 0) return `Starting ${countdownSeconds}s`;
		if (isWaiting) return m.lobby_title();
		if (isLobbyRestarting) return `Restarting ${countdownSeconds}`;
		return m.lobby_game_in_progress();
	}
</script>

<Header title={getTitle()}>
	{#snippet icon()}
		<Users size={32} class="opacity-60" />
	{/snippet}
	{#snippet actions()}
		{#if canRejoinGame}
			<button class="btn btn-accent" onclick={handleRejoin}>{m.lobby_rejoin_game()}</button>
		{/if}
		<button class="btn btn-error" onclick={handleLeave}> {m.lobby_leave_lobby()} </button>
	{/snippet}
	{#snippet subtitle()}
		{#if isWaiting}
			<span>
				{m.lobby_waiting_for_players({
					current: playerCount,
					min: minimumPlayerCount ?? 0,
				})}
			</span>
		{:else if isPendingConnection}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				{m.lobby_connecting()}
			</span>
		{:else if isGameServerLaunching}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				{m.lobby_waiting_for_server()}
			</span>
		{:else if isLaunchingClient}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				{m.lobby_launching_game()}
			</span>
		{:else}
			<span>{playerCount} {m.lobby_players()}</span>
		{/if}
	{/snippet}
</Header>

<div class="flex-1 flex flex-col overflow-hidden bg-base-100 relative">
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

	<div class="relative z-10 flex h-full items-center justify-center gap-12 p-8">
		<div class="flex flex-col gap-2">
			{#each teamLeft as player (player.id)}
				<LobbyPlayerCard
					displayName={player.displayName}
					championIconFolderName={getChampionDisplayName(player.champion)}
					status={getChampionDisplayName(player.champion) || "Not ready"}
				/>
			{/each}
		</div>

		<div class="flex flex-col gap-2">
			{#each teamRight as player (player.id)}
				<LobbyPlayerCard
					displayName={player.displayName}
					championIconFolderName={getChampionDisplayName(player.champion)}
					status={getChampionDisplayName(player.champion) || "Not ready"}
				/>
			{/each}
		</div>
	</div>
</div>
