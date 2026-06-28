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
	} from "$lib/lobby/stores.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createLaunchGameMutation } from "$lib/queries/core";
	import { processesList } from "$lib/stores/processes.svelte";
	import { getMapsForVersion } from "$lib/utils/versions";
	import { onDestroy, onMount } from "svelte";

	const currentMap = $derived(
		lobbyStaticInfo.value?.version ?
			getMapsForVersion(lobbyStaticInfo.value?.version).find(
				(m) => m.id === lobbyState.value.championSelect?.mapId || m.id === lobbyState.value.inGame?.mapId,
			)
		:	undefined,
	);
	const gameRunning = $derived(
		processesList.value.some((p) => p.instance.id === currentInstance.value?.id),
	);

	const launchGameMutation = createLaunchGameMutation();
	const runAfkDetection = $derived(
		!!(
			lobbyState.value.inGame?.gameServerOpen &&
			ownChampion.value &&
			!gameRunning &&
			!launchGameMutation.isPending
		),
	);

	$effect(() => {
		if (
			isGameServerOpen.value &&
			!gameRunning &&
			!launchGameMutation.isPending &&
			ownChampion &&
			!currentInstance.value
		) {
			console.log("Trying to launch the game!");
			handleJoinGame();
		}
		if (!isGameServerOpen.value) {
			currentInstance.value = null;
			//TODO maybe ask user before closing instances
			const openInstances = processesList.value.filter(
				(p) => p.instance.version === lobbyStaticInfo.value?.version,
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
			currentInstance.value = instance;
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
		lobbyPassword.value = password;
		await lobbyManager.joinLobby();
	}
</script>

<svelte:head>
	<title>{m.lobby_title()}</title>
</svelte:head>

<div class="flex flex-col h-full bg-base-100">
	{#if isInChampionSelect.value}
		<!-- TODO: Remove 0.57 placeholder -->
		<LobbyChampionSelect
			teamLeft={teamLeft.value}
			teamRight={teamRight.value}
			{currentMap}
			confirmedChampion={ownChampion.value}
			{handleChampionSelect}
			gameVersion={lobbyStaticInfo.value?.version ?? "0.57"}
			countdownSeconds={currentCountdownSeconds.value}
		/>
	{:else if isInMapVote.value}
		<!-- TODO: Remove 0.57 placeholder -->
		<LobbyMapVote
			{handleLeave}
			playerCount={players.value.length}
			{handleMapSelect}
			votes={lobbyState.value.mapVote?.votes}
			gameVersion={lobbyStaticInfo.value?.version ?? "0.57"}
			gamemode={lobbyStaticInfo.value?.gamemode || "siege"}
			countdownSeconds={currentCountdownSeconds.value}
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
		messages={chatMessages.value}
		disabled={connectionStatus.value !== "connected"}
		{handleSendChatMessage}
	/>
	<LobbyOverlay
		disconnected={connectionStatus.value === "disconnected"}
		gameServerError={!!lobbyState.value.inGame?.gameServerError}
		joinErrorCode={joinErrorCode.value}
		{handlePasswordSubmit}
		{handleJoin}
		lobbyVersion={lobbyStaticInfo.value?.version || "unknown"}
		maxPlayerCount={lobbyStaticInfo.value?.maxPlayers || 0}
		playerCount={players.value.length}
	/>
	<AfkDetector {runAfkDetection} onAfk={handleLeave} />
</div>
