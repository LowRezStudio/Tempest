<script lang="ts">
	import ChampionSelect from "$lib/components/champions/ChampionSelect.svelte";
	import MapSelect from "$lib/components/maps/MapSelect.svelte";
	import { getConnectionToServer, LobbyEvent } from "$lib/rpc";
	import {
		chatMessageStore,
		lobbyHost,
		lobbyPassword,
		playerId,
		playerStore,
		stateStore,
		ticket,
	} from "$lib/stores/lobby";
	import { username } from "$lib/stores/settings";
	import { X } from "@lucide/svelte";
	import { onDestroy, onMount, tick } from "svelte";
	import maps from "$lib/data/maps.json";
	import { goto } from "$app/navigation";
	import type { LobbyEventPlayerJoin } from "$lib/rpc/lobby/lobby_event_player_join";
	import type { LobbyEventPlayerLeave } from "$lib/rpc/lobby/lobby_event_player_leave";
	import type { LobbyEventPlayerUpdate } from "$lib/rpc/lobby/lobby_event_player_update";
	import type { LobbyEventChatMessage } from "$lib/rpc/lobby/lobby_event_chat_message";
	import type { LobbyEventStateUpdate } from "$lib/rpc/lobby/lobby_event_state_update";
	import type { LobbyEventCountdown } from "$lib/rpc/lobby/lobby_event_countdown";
	import {
		createChampionSelectMutation,
		createLeaveLobbyMutation,
		createMapSelectMutation,
		createSendChatMessageMutation,
	} from "$lib/queries/lobby";
	import type { LobbyEventInfo } from "$lib/rpc/lobby/lobby_event_info";

	const client = getConnectionToServer(lobbyHost.get());
	//to close event stream when component destroyed
	const ac = new AbortController();
	let chatboxText = $state<string>("");
	let chatContainer = $state<HTMLDivElement>();
	let chatOpen = $state<boolean>(true);
	let streamConnectionStatus = $state<"connected" | "disconnected" | "pending">("pending");

	//players own team is displayed on the left side
	const ownTeam = $derived($playerStore.find((p) => p.id === playerId.get())?.taskForce);
	const teamLeftPlayers = $derived($playerStore.filter((p) => p.taskForce === ownTeam));
	const teamRightPlayers = $derived($playerStore.filter((p) => p.taskForce !== ownTeam));
	const currentMap = $derived(maps.find((m) => m.id === $stateStore.championSelect?.mapId));

	//TODO deal with errors
	const leaveLobbyMutation = createLeaveLobbyMutation(client);
	const chatMessageMutation = createSendChatMessageMutation(client);
	const mapVoteMutation = createMapSelectMutation(client);
	const championSelectMutation = createChampionSelectMutation(client);

	//update client state based on events from lobby server
	async function actOnEvents() {
		console.log("Starting to listen to event stream");
		streamConnectionStatus = "pending";
		while (!ac.signal.aborted) {
			try {
				const eventStream = client.receiveLobbyEvents({}, { abort: ac.signal });
				for await (const event of eventStream.responses) {
					//marking connected after first event which will be the
					//info event that will always happen
					streamConnectionStatus = "connected";
					await handleEvent(event);
				}
				console.warn("Stream terminated!");
			} catch (e) {
				console.error("Stream error", e);
			}
			streamConnectionStatus = "disconnected";
			if (ac.signal.aborted) return;
			await new Promise((r) => setTimeout(r, 5000));
			console.log("Reconnecting to event stream");
		}
	}
	//Event handlers
	async function handleEvent(event: LobbyEvent) {
		switch (event.event.oneofKind) {
			case "info":
				await handleInfoEvent(event.event.info);
				break;
			case "playerJoin":
				await handlePlayerJoinEvent(event.event.playerJoin);
				break;
			case "playerLeave":
				await handlePlayerLeaveEvent(event.event.playerLeave);
				break;
			case "playerUpdate":
				await handlePlayerUpdateEvent(event.event.playerUpdate);
				break;
			case "chatMessage":
				await handleChatMessageEvent(event.event.chatMessage);
				break;
			case "stateUpdate":
				await handleStateUpdateEvent(event.event.stateUpdate);
				break;
			case "countdown":
				await handleCountdownEvent(event.event.countdown);
				break;
		}
	}
	async function handleInfoEvent(event: LobbyEventInfo) {
		const { players, state } = event;
		playerStore.set(players);
		if (state) {
			stateStore.set(state);
		}
		//player has already been added
		if (players.some((p) => p.id === playerId.get())) return;
		//TODO make a query or mutation?
		const joinResp = await client.joinLobby({
			playerId: playerId.get(),
			playerDisplayName: username.get(),
			password: lobbyPassword.get(),
		});
		console.log(joinResp);
		//storing the ticket that will be used in all requests
		if (joinResp.response.result.oneofKind == "success") {
			ticket.set(joinResp.response.result.success.ticket);
		}
	}
	async function handlePlayerJoinEvent(event: LobbyEventPlayerJoin) {
		const player = event.player;
		if (player) {
			playerStore.set([...$playerStore, player]);
		}
	}
	async function handlePlayerLeaveEvent(event: LobbyEventPlayerLeave) {
		const playerId = event.playerId;
		playerStore.set($playerStore.filter((pl) => pl.id !== playerId));
	}
	async function handlePlayerUpdateEvent(event: LobbyEventPlayerUpdate) {
		const player = event.player;
		playerStore.set($playerStore.map((pl) => (pl.id === player?.id ? player : pl)));
	}
	async function handleChatMessageEvent(event: LobbyEventChatMessage) {
		const message = event.chatMessage;
		if (!message) return;
		const sender = $playerStore.find((p) => p.id === message.authorId);
		//only showing messages of own team and storing the sender username
		//the username is stored at this point because the sender might leave the lobby later
		if (!sender || sender.taskForce !== ownTeam) return;
		chatMessageStore.set([
			...$chatMessageStore,
			{
				content: message.content,
				username: sender.displayName,
				sentAt: message.sentAt,
			},
		]);
		//automatically scroll to bottom if near the bottom (20px thresshold)
		if (
			chatContainer &&
			chatContainer.scrollTop + chatContainer.clientHeight >= chatContainer.scrollHeight - 20
		) {
			await tick();
			chatContainer.scrollTop = chatContainer.scrollHeight;
		}
	}
	async function handleStateUpdateEvent(event: LobbyEventStateUpdate) {
		const state = event.state;
		if (state) {
			stateStore.set(state);
		}
	}
	async function handleCountdownEvent(event: LobbyEventCountdown) {
		const time = event.seconds;
		//TODO implement countdowns
	}

	//Requests to server
	async function handleSendChatMessage() {
		if (!chatboxText.trim()) return;
		chatMessageMutation.mutate(chatboxText);
		chatboxText = "";
	}
	async function handleChampionSelect(championName: string) {
		championSelectMutation.mutate(championName);
	}
	async function handleMapSelect(mapId: string) {
		mapVoteMutation.mutate(mapId);
	}
	async function handleLeave() {
		//do not care about result
		leaveLobbyMutation.mutate();
		lobbyHost.set("");
		lobbyPassword.set("");
		ticket.set("");
		goto("/servers");
	}
	//We only want to call this once, not every time the dependencies change
	//therefore not using $effect
	onMount(() => {
		actOnEvents();
	});

	//closing the event stream
	onDestroy(() => {
		console.log("Cleaning up the event stream");
		ac.abort();
	});
</script>

<svelte:head>
	<title>Lobby</title>
</svelte:head>

<div class="flex flex-col h-full bg-base-100">
	<div class="absolute w-full h-full z-11 pointer-events-none">
		<!-- Champion lists on both sides -->
		<div class="left-0 pt-30 h-full absolute flex flex-col p-5 rounded-md">
			{#each teamLeftPlayers as player (player.id)}
				<div class="flex items-center h-25 w-60 bg-base-300 gap-3 pl-3">
					<img
						src={`/champions/${player.champion || "Generic"}/icon.webp`}
						alt={player.champion}
						class="w-20 h-20 rounded-full border-2"
						loading="lazy"
					/>
					<div>
						<p>{player.displayName}</p>
						<p>
							{$stateStore.mapVote && player.id in $stateStore.mapVote.votes ?
								"Voted"
							:	player.champion}
						</p>
					</div>
				</div>
			{/each}
		</div>
		<div class="right-0 pt-30 h-full absolute flex flex-col p-5 rounded-md">
			{#each teamRightPlayers as player (player.id)}
				<div class="flex items-center h-25 w-60 bg-base-300 gap-3 pl-3">
					<img
						src={`/champions/${player.champion || "Generic"}/icon.webp`}
						alt={player.champion}
						class="w-20 h-20 rounded-full border-2"
						loading="lazy"
					/>
					<div>
						<p>{player.displayName}</p>
						<p>
							{$stateStore.mapVote && player.id in $stateStore.mapVote.votes ?
								"Voted"
							:	player.champion}
						</p>
					</div>
				</div>
			{/each}
		</div>
		<!-- Chat -->
		{#if chatOpen}
			<div class="absolute bottom-0 m-6 bg-base-300 p-4 pt-7 rounded-md pointer-events-auto">
				<button
					class="absolute right-0 top-0 p-2"
					onclick={() => {
						chatOpen = false;
					}}><X class="w-5 h-5" /></button
				>
				<div
					class="max-w-100 overflow-auto max-h-[220px] wrap-break-word"
					bind:this={chatContainer}
				>
					{#each $chatMessageStore as msg (msg.sentAt)}
						<div>
							{msg.username}: {msg.content}
						</div>
					{/each}
				</div>
				<input
					id="chatbox"
					type="text"
					class="input w-100 bg-base-300"
					disabled={streamConnectionStatus !== "connected"}
					placeholder="Type into the chat..."
					max={100}
					autocomplete="off"
					bind:value={chatboxText}
					onkeydown={(e) => e.key == "Enter" && handleSendChatMessage()}
				/>
			</div>
		{:else}
			<button
				class="btn btn-accent absolute bottom-0 m-6 pointer-events-auto"
				onclick={() => {
					chatOpen = true;
					tick().then(() => {
						if (!chatContainer) return;
						chatContainer.scrollTop = chatContainer.scrollHeight;
					});
				}}>Chat</button
			>
		{/if}

		<!--Map indicator-->
		{#if $stateStore.championSelect}
			<div class="absolute bottom-25 right-0 bg-base-300 w-100">
				<h2>Playing map: {currentMap?.displayName}</h2>
				<img src={currentMap?.iconPath} alt={currentMap?.displayName} />
			</div>
		{/if}

		<!-- Error window-->
		{#if streamConnectionStatus === "disconnected"}
			<div
				class="absolute w-full h-full flex items-center justify-center z-20 pointer-events-auto"
			>
				<div
					class="w-100 h-40 bg-base-300 flex items-center justify-center rounded-lg flex-col"
				>
					<p>Unable to connect to the lobby server!</p>
					<p>
						<span class="loading loading-spinner loading-sm"></span> Trying to reconnect...
					</p>
				</div>
			</div>
		{/if}

		<!-- Leave button -->
		<button
			class="btn btn-danger absolute bottom-0 right-0 bg-red-500 m-6 z-30 w-30 h-12 pointer-events-auto"
			onclick={handleLeave}
		>
			Leave
		</button>
	</div>
	<!-- Champion selector or map selector -->
	{#if $stateStore.championSelect}
		<ChampionSelect
			confirmedChampionName={$playerStore.find((p) => p.id == playerId.get())?.champion}
			onselect={(champ) => handleChampionSelect(champ.name)}
		/>
	{:else if $stateStore.mapVote}
		<MapSelect
			onselect={(map) => handleMapSelect(map.id)}
			selectMode="vote"
			votes={$stateStore.mapVote?.votes}
		/>
	{:else}
		<!-- Background video-->
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
		<!-- Header  -->
		{#if $stateStore.waiting || $stateStore.inGame}
			<div class="p-8 text-center absolute w-full">
				<h1
					class="text-4xl font-bold text-white"
					style="text-shadow: 0 4px 12px rgba(0,0,0,0.8), 0 2px 4px rgba(0,0,0,0.9);"
				>
					{$stateStore.waiting ?
						`Waiting for players ${$playerStore.length}/${$stateStore.waiting.minPlayers}`
					:	"Game in progress"}
				</h1>
			</div>
		{/if}
	{/if}
</div>
