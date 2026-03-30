<script lang="ts">
	import { Users } from "@lucide/svelte";
	import champions from "$lib/data/champions.json";
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
		if (isWaiting) return "Lobby";
		if (isLobbyRestarting) return `Restarting ${countdownSeconds}`;
		return "Game in Progress";
	}
</script>

<Header title={getTitle()}>
	{#snippet icon()}
		<Users size={32} class="opacity-60" />
	{/snippet}
	{#snippet actions()}
		{#if canRejoinGame}
			<button class="btn btn-accent" onclick={handleRejoin}>Rejoin Game</button>
		{/if}
		<button class="btn btn-error" onclick={handleLeave}> Leave Lobby </button>
	{/snippet}
	{#snippet subtitle()}
		{#if isWaiting}
			<span>
				Waiting for players {playerCount}/{minimumPlayerCount}
			</span>
		{:else if isPendingConnection}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				Connecting
			</span>
		{:else if isGameServerLaunching}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				Waiting for server to start
			</span>
		{:else if isLaunchingClient}
			<span class="inline-flex items-center gap-2">
				<span class="loading loading-spinner loading-xs"></span>
				Launching Paladins
			</span>
		{:else}
			<span>{playerCount} players</span>
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
