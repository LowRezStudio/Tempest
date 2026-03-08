<script lang="ts">
	import { LogOut, MessageCircle, Users, X } from "@lucide/svelte";
	import ChampionSelect from "$lib/components/champions/ChampionSelect.svelte";
	import MapSelect from "$lib/components/maps/MapSelect.svelte";
	import maps from "$lib/data/maps.json";
	import { lobbyManager } from "$lib/lobby/lobby-manager";
	import {
		chatMessages,
		connectionStatus,
		isInChampionSelect,
		isInGame,
		isInMapVote,
		isWaiting,
		state as lobbyState,
		playerId,
		players,
		teamLeft,
		teamRight,
	} from "$lib/lobby/stores";
	import { onDestroy, onMount, tick } from "svelte";

	let chatboxText = $state<string>("");
	let chatContainer = $state<HTMLDivElement>();
	let chatOpen = $state<boolean>(false);

	const currentMap = $derived(maps.find((m) => m.id === $lobbyState.championSelect?.mapId));

	onMount(() => {
		lobbyManager.connect();
	});

	onDestroy(() => {
		if (!lobbyManager.isConnected()) {
			lobbyManager.cleanup();
		}
	});

	async function handleSendChatMessage() {
		await lobbyManager.sendChatMessage(chatboxText);
		chatboxText = "";
	}

	async function handleChampionSelect(championName: string) {
		await lobbyManager.selectChampion(championName);
	}

	async function handleMapSelect(mapId: string) {
		await lobbyManager.voteForMap(mapId);
	}

	async function handleLeave() {
		await lobbyManager.leaveLobby();
	}

	function getPlayerStatus(player: { id: string; champion?: string }): string {
		if ($lobbyState.mapVote && player.id in $lobbyState.mapVote.votes) {
			return "Voted";
		}
		return player.champion || "Not ready";
	}
</script>

<svelte:head>
	<title>Lobby</title>
</svelte:head>

<div class="flex flex-col h-full bg-base-100">
	{#if $isInChampionSelect}
		<div class="relative h-full w-full">
			<div class="absolute inset-0 z-10">
				<div class="absolute top-0 left-0 p-4 z-20">
					<div class="flex flex-col gap-2">
						{#each $teamLeft as player (player.id)}
							<div
								class="flex items-center gap-3 bg-base-200/90 backdrop-blur-xs rounded-lg p-2"
							>
								<img
									src={`/champions/${player.champion || "Generic"}/icon.webp`}
									alt={player.champion}
									class="w-12 h-12 rounded-full ring-2 ring-base-300"
									loading="lazy"
								/>
								<div class="min-w-0">
									<p class="font-semibold truncate">{player.displayName}</p>
									<p class="text-sm opacity-70">{getPlayerStatus(player)}</p>
								</div>
							</div>
						{/each}
					</div>
				</div>

				<div class="absolute top-0 right-0 p-4 z-20">
					<div class="flex flex-col gap-2">
						{#each $teamRight as player (player.id)}
							<div
								class="flex items-center gap-3 bg-base-200/90 backdrop-blur-xs rounded-lg p-2"
							>
								<img
									src={`/champions/${player.champion || "Generic"}/icon.webp`}
									alt={player.champion}
									class="w-12 h-12 rounded-full ring-2 ring-base-300"
									loading="lazy"
								/>
								<div class="min-w-0">
									<p class="font-semibold truncate">{player.displayName}</p>
									<p class="text-sm opacity-70">{getPlayerStatus(player)}</p>
								</div>
							</div>
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
				confirmedChampionName={$players.find((p) => p.id == playerId.get())?.champion}
				onselect={(champ) => handleChampionSelect(champ.name)}
			/>
		</div>
	{:else if $isInMapVote}
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
				<div class="bg-base-200/90 backdrop-blur-xs">
					<div class="px-4 py-3">
						<div class="flex items-center justify-between">
							<div class="flex items-center gap-3">
								<div
									class="w-16 h-16 rounded-xl bg-base-300 flex items-center justify-center shrink-0"
								>
									<Users size={32} class="opacity-60" />
								</div>
								<div>
									<h1 class="text-2xl font-bold mb-1">Map Vote</h1>
									<div
										class="flex items-center gap-3 text-sm text-base-content/70"
									>
										<span>{$players.length} players</span>
									</div>
								</div>
							</div>

							<div class="flex items-center gap-2">
								<button class="btn btn-error" onclick={handleLeave}>
									<LogOut size={18} />
									Leave Lobby
								</button>
							</div>
						</div>
					</div>
				</div>

				<div class="flex-1 overflow-y-auto">
					<MapSelect
						onselect={(map) => handleMapSelect(map.id)}
						selectMode="vote"
						votes={$lobbyState.mapVote?.votes}
					/>
				</div>
			</div>
		</div>
	{:else}
		<div class="bg-base-200">
			<div class="px-4 py-3">
				<div class="flex items-center justify-between">
					<div class="flex items-center gap-3">
						<div
							class="w-16 h-16 rounded-xl bg-base-300 flex items-center justify-center shrink-0"
						>
							<Users size={32} class="opacity-60" />
						</div>
						<div>
							<h1 class="text-2xl font-bold mb-1">
								{$isInGame ? "Game in Progress" : "Lobby"}
							</h1>
							<div class="flex items-center gap-3 text-sm text-base-content/70">
								{#if $isWaiting}
									<span>
										Waiting for players {$players.length}/{$lobbyState.waiting
											?.minPlayers}
									</span>
								{:else}
									<span>{$players.length} players</span>
								{/if}
								{#if $connectionStatus === "pending"}
									<span class="inline-flex items-center gap-2">
										<span class="loading loading-spinner loading-xs"></span>
										Connecting
									</span>
								{/if}
							</div>
						</div>
					</div>

					<div class="flex items-center gap-2">
						<button class="btn btn-error" onclick={handleLeave}> Leave Lobby </button>
					</div>
				</div>
			</div>
		</div>

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
					{#each $teamLeft as player (player.id)}
						<div
							class="flex items-center gap-3 bg-base-200/90 backdrop-blur-xs rounded-lg p-2"
						>
							<img
								src={`/champions/${player.champion || "Generic"}/icon.webp`}
								alt={player.champion}
								class="w-12 h-12 rounded-full ring-2 ring-base-300"
								loading="lazy"
							/>
							<div class="min-w-0">
								<p class="font-semibold truncate">{player.displayName}</p>
								<p class="text-sm opacity-70">{player.champion || "Not ready"}</p>
							</div>
						</div>
					{/each}
				</div>

				<div class="flex flex-col gap-2">
					{#each $teamRight as player (player.id)}
						<div
							class="flex items-center gap-3 bg-base-200/90 backdrop-blur-xs rounded-lg p-2"
						>
							<img
								src={`/champions/${player.champion || "Generic"}/icon.webp`}
								alt={player.champion}
								class="w-12 h-12 rounded-full ring-2 ring-base-300"
								loading="lazy"
							/>
							<div class="min-w-0">
								<p class="font-semibold truncate">{player.displayName}</p>
								<p class="text-sm opacity-70">{player.champion || "Not ready"}</p>
							</div>
						</div>
					{/each}
				</div>
			</div>
		</div>
	{/if}

	{#if chatOpen}
		<div
			class="absolute bottom-4 left-4 z-20 w-96 bg-base-200/95 backdrop-blur-xs rounded-lg shadow-xl flex flex-col max-h-96"
		>
			<div class="px-3 py-2 border-b border-base-300 flex items-center justify-between">
				<div class="flex items-center gap-2">
					<MessageCircle size={16} />
					<span class="font-semibold text-sm">Team Chat</span>
				</div>
				<button
					class="btn btn-ghost btn-sm btn-square"
					onclick={() => (chatOpen = false)}
					aria-label="Close chat"
				>
					<X size={14} />
				</button>
			</div>

			<div class="flex-1 overflow-y-auto p-3 min-h-0" bind:this={chatContainer}>
				{#if $chatMessages.length === 0}
					<p class="text-sm opacity-50 text-center py-2">No messages yet</p>
				{:else}
					<div class="flex flex-col gap-1.5">
						{#each $chatMessages as msg (msg.sentAt)}
							<div class="text-sm">
								<span class="font-semibold">{msg.username}</span>
								<span class="opacity-70">: {msg.content}</span>
							</div>
						{/each}
					</div>
				{/if}
			</div>

			<div class="p-2 border-t border-base-300">
				<input
					type="text"
					class="input input-bordered input-sm w-full"
					disabled={$connectionStatus !== "connected"}
					placeholder="Type a message..."
					maxlength={100}
					autocomplete="off"
					bind:value={chatboxText}
					onkeydown={(e) => e.key == "Enter" && handleSendChatMessage()}
				/>
			</div>
		</div>
	{:else}
		<button
			class="btn btn-accent absolute bottom-4 left-4 z-20"
			onclick={() => {
				chatOpen = true;
				tick().then(() => {
					if (!chatContainer) return;
					chatContainer.scrollTop = chatContainer.scrollHeight;
				});
			}}
		>
			<MessageCircle size={18} />
			Chat
		</button>
	{/if}

	{#if $connectionStatus === "disconnected"}
		<div class="absolute inset-0 flex items-center justify-center z-30 bg-black/50">
			<div class="modal-box flex flex-col items-center gap-4">
				<p class="font-semibold text-lg">Connection Lost</p>
				<p class="text-sm opacity-70 text-center">
					Unable to connect to the lobby server. Reconnecting...
				</p>
				<span class="loading loading-spinner loading-md"></span>
			</div>
		</div>
	{/if}
</div>
