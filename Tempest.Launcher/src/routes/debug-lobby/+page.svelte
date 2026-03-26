<script lang="ts">
	/*
The purpose of this page is to be a development tool
Makes it possible to add players, vote maps and select champions
*/
	import { GrpcWebFetchTransport } from "@protobuf-ts/grpcweb-transport";
	import { chatMessages, debugPlayersStore, players, state } from "$lib/lobby/stores";
	import { LobbyClient } from "$lib/rpc/lobby/lobby_service.client";
	import { onDestroy, onMount } from "svelte";
	import type { RpcOptions } from "@protobuf-ts/runtime-rpc";

	const transport = new GrpcWebFetchTransport({
		baseUrl: "http://localhost:50051",
		format: "binary", //not using tauri fetch here because this is only used in the browser
	});
	const client = new LobbyClient(transport);
	//to close event stream when component destroyed
	const ac = new AbortController();
	//common logic for adding the x-ticket header to requests
	//protobug-ts interceptor
	const rpcOptions = function (id: string): RpcOptions {
		const ticket = $debugPlayersStore.get(id) || "";
		return {
			interceptors: [
				{
					interceptUnary(next, method, input, options) {
						if (!options.meta) {
							options.meta = {};
						}
						options.meta["x-ticket"] = ticket;
						return next(method, input, options);
					},
				},
			],
		};
	};
	const champions: string[] = [
		"Androxus",
		"Ash",
		"Barik",
		"Bomb King",
		"Buck",
		"Cassie",
		"Drogoz",
		"Evie",
		"Fernando",
		"Grohk",
		"Grover",
		"Inara",
		"Jenos",
		"Kinessa",
		"Lex",
		"Lian",
		"Maeve",
		"Makoa",
		"Mal'Damba",
		"Pip",
		"Ruckus",
		"Seris",
		"Sha Lin",
		"Skye",
		"Strix",
		"Talus",
		"Terminus",
		"Torvald",
		"Tyra",
		"Viktor",
		"Vivian",
		"Willo",
		"Zhin",
	];
	const maps = [
		{
			displayName: "Serpent Beach",
			id: "SG_Serpentbeach_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Beach.webp",
		},
		{
			displayName: "Reworked Serpent Beach",
			id: "SG_SerpentBeach_RW_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Beach.webp",
		},

		{
			displayName: "Jaguar Falls",
			id: "SG_Jaguarfalls_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Temple.webp",
		},
		{
			displayName: "Reworked Jaguar Falls",
			id: "SG_JaguarFalls_Rework_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Temple.webp",
		},

		{
			displayName: "Fish Market",
			id: "SG_Fishmarket_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Village.webp",
		},

		{
			displayName: "Frog Isle",
			id: "SG_FrogIsle_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Isle.webp",
		},
		{
			displayName: "Frog Isle V2",
			id: "SG_FrogIsle_V2_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Isle.webp",
		},

		{
			displayName: "Ice Mines",
			id: "SG_Icemines_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_NRMines.webp",
		},

		{
			displayName: "Frozen Guard",
			id: "SG_FrozenGuard_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_NRIgloo.webp",
		},
		{
			displayName: "Reworked Frozen Guard",
			id: "SGA_SG_FrozenGuard_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_NRIgloo.webp",
		},

		{
			displayName: "Stone Keep",
			id: "SG_Stonekeep_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Castle.webp",
		},
		{
			displayName: "Stone Keep V2",
			id: "SG_StoneKeep_V2_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Castle.webp",
		},
		{
			displayName: "Stone Keep Day",
			id: "SG_StoneKeep_V2_DAY_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Castle.webp",
		},

		{
			displayName: "Brightmarsh",
			id: "SG_Brightmarsh_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_Atrium.webp",
		},

		{
			displayName: "Ascension Peak",
			id: "SG_Ascensionpeak_P",
			mode: "siege",
			iconPath: "/loading-screens/Loading_AscensionPeak.webp",
		},
	];
	async function actOnEvents() {
		console.log("Starting to listen to stream");
		const eventStream = client.receiveLobbyEvents(
			{},
			{
				abort: ac.signal,
			},
		);
		for await (const event of eventStream.responses) {
			if (event.event.oneofKind === "playerJoin") {
				const player = event.event.playerJoin.player;
				if (player) {
					players.set([...$players, player]);
				}
			} else if (event.event.oneofKind === "playerLeave") {
				const playerId = event.event.playerLeave.playerId;
				players.set($players.filter((pl) => pl.id !== playerId));
			} else if (event.event.oneofKind === "playerUpdate") {
				const player = event.event.playerUpdate.player;
				players.set($players.map((pl) => (pl.id === player?.id ? player : pl)));
			} else if (event.event.oneofKind === "chatMessage") {
				const message = event.event.chatMessage.chatMessage;
				if (message) {
					const sender = $players.find((p) => p.id === message.authorId);
					chatMessages.set([
						...$chatMessages,
						{
							content: message.content,
							username: sender?.displayName || "unknown",
							sentAt: message.sentAt,
						},
					]);
				}
			} else if (event.event.oneofKind === "stateUpdate") {
				const eventState = event.event.stateUpdate.state;
				if (eventState) {
					state.set(eventState);
				}
			} else if (event.event.oneofKind === "countdown") {
				const time = event.event.countdown.seconds;
			} else if (event.event.oneofKind === "info") {
				const { players: eventPlayers, state: eventState } = event.event.info;
				players.set(eventPlayers);
				if (eventState) {
					state.set(eventState);
				}
			}
			console.log(event);
		}
		console.log("Stream terminated!");
	}
	async function sendChatMessage(content: string, id: string) {
		const resp = await client.sendChatMessage(
			{
				content: content,
			},
			rpcOptions(id),
		);
		console.log(resp);
	}
	async function handleChampionSelect(champion: string, id: string) {
		console.log("Selected", champion);
		const resp = await client.championSelect(
			{
				name: champion,
			},
			rpcOptions(id),
		);
		console.log(resp);
	}
	async function handleMapSelect(map: string, id: string) {
		if (!map) return;
		console.log("Map selected:", map);
		const resp = await client.mapVote(
			{
				mapId: map,
			},
			rpcOptions(id),
		);
		console.log(resp);
	}
	async function handleLeave(id: string) {
		const resp = await client.leaveLobby({}, rpcOptions(id));
		console.log(resp);
	}
	onMount(() => {
		actOnEvents();
	});

	async function createNewPlayer() {
		const id = String(Math.floor(Math.random() * 1000000) + 1);
		const joinResp = await client.joinLobby({
			playerId: id,
			playerDisplayName: `Test ${id}`,
			password: "",
		});
		console.log(joinResp);
		if (joinResp.response.result.oneofKind === "success") {
			const ticket = joinResp.response.result.success.ticket;
			$debugPlayersStore.set(id, ticket);
		}
	}
	//closing the event stream
	onDestroy(() => {
		console.log("Calling destroy");
		ac.abort();
	});
</script>

<svelte:head>
	<title>Lobby DEBUG</title>
</svelte:head>

<div class="flex flex-col h-full bg-base-100 p-6">
	<div class="flex gap-3 items-center">
		<button onclick={createNewPlayer} class="btn">New player</button>
		<p>State {JSON.stringify($state)}</p>
	</div>
	<table class="table table-zebra">
		<thead>
			<tr>
				<th>Name</th>
				<th>Id</th>
				<th>Team</th>
				<th>Champ</th>
				<th>Vote map</th>
				<th>Send champ</th>
				<th>Send msg</th>
				<th>Leave</th>
			</tr>
		</thead>
		<tbody>
			{#each $players as player (player.id)}
				<tr>
					<td>{player.displayName}</td>
					<td>{player.id}</td>
					<td>{player.taskForce}</td>
					<td>{player.champion}</td>
					<td>
						{#if $debugPlayersStore.has(player.id)}
							<select
								onchange={(e) =>
									handleMapSelect(
										(e.target as HTMLSelectElement)?.value,
										player.id,
									)}
							>
								<option value="">Select map...</option>
								{#each maps as map (map.id)}
									<option value={map.id} label={map.displayName}></option>
								{/each}
							</select>
						{/if}
					</td>
					<td>
						{#if $debugPlayersStore.has(player.id)}
							<select
								onchange={(e) =>
									handleChampionSelect(
										(e.target as HTMLSelectElement)?.value,
										player.id,
									)}
							>
								<option value="">Select champion...</option>
								{#each champions as champion (champion)}
									<option value={champion} label={champion}></option>
								{/each}
							</select>
						{/if}
					</td>
					<td>
						{#if $debugPlayersStore.has(player.id)}
							<input
								type="text"
								class="input w-30"
								onkeydown={(e) =>
									e.key == "Enter" &&
									sendChatMessage(
										(e.target as HTMLInputElement).value,
										player.id,
									)}
							/>
						{/if}
					</td>
					<td>
						{#if $debugPlayersStore.has(player.id)}
							<button class="btn" onclick={() => handleLeave(player.id)}>Leave</button
							>
						{/if}
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>
