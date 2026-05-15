<script lang="ts">
	import { goto } from "$app/navigation";
	import AfkDetector from "$lib/components/lobby/AfkDetector.svelte";
	import LobbyChampionSelect from "$lib/components/lobby/LobbyChampionSelect.svelte";
	import LobbyChat from "$lib/components/lobby/LobbyChat.svelte";
	import LobbyMapVote from "$lib/components/lobby/LobbyMapVote.svelte";
	import LobbyOverlay from "$lib/components/lobby/LobbyOverlay.svelte";
	import LobbyWaiting from "$lib/components/lobby/LobbyWaiting.svelte";
	import { killGame } from "$lib/core";
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
		ownChampion,
		playerId,
		players,
		teamLeft,
		teamRight,
	} from "$lib/lobby/stores";
	import { m } from "$lib/paraglide/messages";
	import { createLaunchGameMutation } from "$lib/queries/core";
	import { processesList } from "$lib/stores/processes";
	import { getMapsForVersion } from "$lib/utils/versions";
	import { onDestroy, onMount } from "svelte";

	const currentMap = $derived(
		$lobbyStaticInfo?.version ?
			getMapsForVersion($lobbyStaticInfo?.version).find(
				(m) => m.id === $lobbyState.championSelect?.mapId,
			)
		:	undefined,
	);
	const gameRunning = $derived(
		$processesList.some((p) => p.instance.id === $currentInstance?.id),
	);

	const launchGameMutation = createLaunchGameMutation();
	const runAfkDetection = $derived(
		!!(
			$lobbyState.inGame?.gameServerOpen &&
			$ownChampion &&
			!gameRunning &&
			!launchGameMutation.isPending
		),
	);

	$effect(() => {
		if (
			$isGameServerOpen &&
			!gameRunning &&
			!launchGameMutation.isPending &&
			ownChampion &&
			!$currentInstance
		) {
			console.log("Trying to launch the game!");
			handleJoinGame();
		}
		if (!$isGameServerOpen) {
			currentInstance.set(null);
			//TODO maybe ask user before closing instances
			const openInstances = $processesList.filter(
				(p) => p.instance.version === $lobbyStaticInfo?.version,
			);
			for (const i of openInstances) {
				killGame(i.instance);
			}
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
	async function handleJoinInProgress(championName: string) {
		await lobbyManager.selectChampion(championName);
		handleJoinGame();
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
	function handleJoinGame() {
		const instance = lobbyManager.getLaunchGameInstance();
		if (instance) {
			console.log("Found instance. Launching game!");
			currentInstance.set(instance);
			launchGameMutation.mutate(instance);
		} else {
			console.error("Unable to create instance");
			console.error(
				"Game running",
				gameRunning,
				"Champion",
				ownChampion,
				"LobbyInfo",
				lobbyStaticInfo,
			);
		}
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
		<!-- TODO: Remove 0.57 placeholder -->
		<LobbyChampionSelect
			teamLeft={$teamLeft}
			teamRight={$teamRight}
			{currentMap}
			confirmedChampion={$ownChampion}
			{handleChampionSelect}
			gameVersion={$lobbyStaticInfo?.version ?? "0.57"}
			countdownSeconds={$currentCountdownSeconds}
		/>
	{:else if $isInMapVote}
		<!-- TODO: Remove 0.57 placeholder -->
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
			handleRejoinGame={handleJoinGame}
			handleRejoinLobby={handleJoin}
			{handleJoinInProgress}
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
	<AfkDetector {runAfkDetection} onAfk={handleLeave} />
</div>
