<script lang="ts">
	import { goto } from "$app/navigation";
	import LobbyChampionSelect from "$lib/components/lobby/LobbyChampionSelect.svelte";
	import LobbyChat from "$lib/components/lobby/LobbyChat.svelte";
	import LobbyMapVote from "$lib/components/lobby/LobbyMapVote.svelte";
	import LobbyOverlay from "$lib/components/lobby/LobbyOverlay.svelte";
	import LobbyWaiting from "$lib/components/lobby/LobbyWaiting.svelte";
	import maps from "$lib/data/maps.json";
	import { lobbyManager } from "$lib/lobby/lobby-manager";
	import {
		chatMessages,
		connectionStatus,
		currentCountdownSeconds,
		currentInstance,
		isGameServerOpen,
		isInChampionSelect,
		isInGame,
		isInMapVote,
		isWaiting,
		joinErrorCode,
		lobbyPassword,
		state as lobbyState,
		lobbyStaticInfo,
		playerId,
		players,
		teamLeft,
		teamRight,
	} from "$lib/lobby/stores";
	import { m } from "$lib/paraglide/messages";
	import { createLaunchGameMutation } from "$lib/queries/core";
	import { processesList } from "$lib/stores/processes";
	import { onDestroy, onMount } from "svelte";

	const currentMap = $derived(maps.find((m) => m.id === $lobbyState.championSelect?.mapId));
	const gameRunning = $derived(
		$processesList.some((p) => p.instance.id === $currentInstance?.id),
	);

	const ownChampion = $derived($players.find((p) => p.id == playerId.get())?.champion);

	const launchGameMutation = createLaunchGameMutation();

	$effect(() => {
		const instance = lobbyManager.getLaunchGameInstance();
		if (instance && $isGameServerOpen && !launchGameMutation.isPending && !$currentInstance) {
			currentInstance.set(instance);
			launchGameMutation.mutate(instance);
		}
		if (!$isGameServerOpen) {
			currentInstance.set(null);
		}
	});
	onMount(() => {
		lobbyManager.connect();
	});

	onDestroy(() => {
		if (!lobbyManager.isConnected()) {
			lobbyManager.cleanup();
		}
	});

	async function handleSendChatMessage(message: string) {
		await lobbyManager.sendChatMessage(message);
	}

	async function handleChampionSelect(championName: string) {
		await lobbyManager.selectChampion(championName);
	}

	async function handleMapSelect(mapId: string) {
		await lobbyManager.voteForMap(mapId);
	}

	async function handleLeave() {
		await lobbyManager.leaveLobby();
		goto("/servers");
	}
	async function handleJoin() {
		await lobbyManager.joinLobby();
	}
	function handleRejoin() {
		if (!$currentInstance) return;
		launchGameMutation.mutate($currentInstance);
	}
	async function handlePasswordSubmit(password: string) {
		lobbyPassword.set(password);
		await lobbyManager.joinLobby();
	}
</script>

<svelte:head>
	<title>{m.lobby_title()}</title>
</svelte:head>

<div class="flex flex-col h-full bg-base-100">
	{#if $isInChampionSelect}
		<LobbyChampionSelect
			teamLeft={$teamLeft}
			teamRight={$teamRight}
			{currentMap}
			confirmedChampion={ownChampion}
			{handleChampionSelect}
			gameVersion={$lobbyStaticInfo?.version ?? "0.57"}
			countdownSeconds={$currentCountdownSeconds}
		/>
	{:else if $isInMapVote}
		<LobbyMapVote
			{handleLeave}
			playerCount={$players.length}
			{handleMapSelect}
			votes={$lobbyState.mapVote?.votes}
			gameVersion={$lobbyStaticInfo?.version ?? "0.57"}
			gamemode={$lobbyStaticInfo?.gamemode || "siege"}
			countdownSeconds={$currentCountdownSeconds}
		/>
	{:else}
		<LobbyWaiting
			isGameInProgress={$isInGame}
			isWaiting={$isWaiting}
			isPendingConnection={$connectionStatus === "pending"}
			isGameServerLaunching={!$isGameServerOpen && !$lobbyState.inGame?.gameServerError}
			isLobbyRestarting={!!$lobbyState.inGame?.gameServerFinishedRunning}
			isLaunchingClient={launchGameMutation.isPending}
			canRejoinGame={!!(
				!gameRunning &&
				!launchGameMutation.isPending &&
				$isGameServerOpen &&
				$currentInstance
			)}
			playerCount={$players.length}
			minimumPlayerCount={$lobbyState.waiting?.minPlayers || 0}
			teamLeft={$teamLeft}
			teamRight={$teamRight}
			countdownSeconds={$currentCountdownSeconds}
			{handleRejoin}
			{handleLeave}
		/>
	{/if}
	<LobbyChat
		messages={$chatMessages}
		disabled={$connectionStatus !== "connected"}
		{handleSendChatMessage}
	/>
	<LobbyOverlay
		disconnected={$connectionStatus === "disconnected"}
		gameServerError={!!$lobbyState.inGame?.gameServerError}
		joinErrorCode={$joinErrorCode}
		{handlePasswordSubmit}
		{handleJoin}
		lobbyVersion={$lobbyStaticInfo?.version || "unknown"}
		maxPlayerCount={$lobbyStaticInfo?.maxPlayers || 0}
		playerCount={$players.length}
	/>
</div>
