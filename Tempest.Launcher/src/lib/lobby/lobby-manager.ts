import { AuthMethod, getConnectionToServer, LobbyEvent, Timestamp } from "$lib/rpc";
import { JoinLobbyErrorCode } from "$lib/rpc/lobby/join_lobby_error_code";
import { instanceMap } from "$lib/stores/instance.svelte";
/* eslint-disable-next-line no-unused-vars */
import { processesList } from "$lib/stores/processes.svelte";
import { username } from "$lib/stores/settings.svelte";
import { JoinLobbyClientErrorCode } from "$lib/types/lobby";
import {
	chatMessages,
	connectionStatus,
	currentCountdownSeconds,
	joinErrorCode,
	lobbyHost,
	lobbyPassword,
	lobbyStaticInfo,
	mostRecentLobbyConnectionTime,
	ownTeam,
	playerId,
	players,
	resetLobbyState,
	state,
	ticket,
} from "./stores.svelte";
import type { LobbyEventChatMessage } from "$lib/rpc/lobby/lobby_event_chat_message";
import type { LobbyEventCountdown } from "$lib/rpc/lobby/lobby_event_countdown";
import type { LobbyEventInfo } from "$lib/rpc/lobby/lobby_event_info";
import type { LobbyEventPlayerJoin } from "$lib/rpc/lobby/lobby_event_player_join";
import type { LobbyEventPlayerLeave } from "$lib/rpc/lobby/lobby_event_player_leave";
import type { LobbyEventPlayerUpdate } from "$lib/rpc/lobby/lobby_event_player_update";
import type { LobbyEventStateUpdate } from "$lib/rpc/lobby/lobby_event_state_update";
import type { LobbyClient } from "$lib/rpc/lobby/lobby_service.client";
import type { Instance } from "$lib/types/instance";

class LobbyManager {
	private client: LobbyClient | null = null;
	private abortController: AbortController | null = null;
	private countdownTimerInterval?: number;
	private countdown?: LobbyEventCountdown;

	connect(): void {
		if (this.abortController && !this.abortController.signal.aborted) {
			console.log("Already connected to lobby");
			return;
		}
		const host = lobbyHost.value;
		if (!host) {
			console.error("Cannot connect: lobbyHost is empty");
			return;
		}
		console.log("Connecting to lobby:", host);
		this.client = getConnectionToServer(host);
		this.abortController = new AbortController();
		mostRecentLobbyConnectionTime.value = new Date().toISOString();
		clearInterval(this.countdownTimerInterval);
		this.countdownTimerInterval = setInterval(() => {
			if (!this.countdown || !this.countdown.startTime) return;
			const start = this.countdown.startTime;
			const now = Timestamp.now();
			const elapsed = Number(now.seconds - start.seconds) + (now.nanos - start.nanos) / 1e9;
			const secondsLeft = this.countdown.seconds - Math.floor(elapsed);
			currentCountdownSeconds.value = secondsLeft;
		}, 250);
		void this.startEventStream();
	}

	private getClient(): LobbyClient {
		if (!this.client) {
			throw new Error("LobbyManager not connected");
		}
		return this.client;
	}

	disconnect(): void {
		if (!this.abortController) return;
		this.abortController.abort();
		this.abortController = null;
		connectionStatus.value = "pending";
		clearInterval(this.countdownTimerInterval);
		console.log("Lobby disconnected");
	}

	private async startEventStream(): Promise<void> {
		console.log("Starting to listen to event stream");
		connectionStatus.value = "pending";

		while (this.abortController !== null && !this.abortController?.signal.aborted) {
			try {
				const eventStream = this.getClient().receiveLobbyEvents(
					{ playerId: playerId.value },
					{ abort: this.abortController!.signal },
				);
				console.log("Listening to lobby events");
				for await (const event of eventStream.responses) {
					connectionStatus.value = "connected";
					await this.handleEvent(event);
				}
				console.warn("Stream terminated!");
			} catch (error) {
				console.error("Stream error", error);
			}
			connectionStatus.value = "disconnected";
			if (this.abortController == null || this.abortController?.signal.aborted) return;
			await new Promise((r) => setTimeout(r, 5000));
			if (this.abortController == null || this.abortController?.signal.aborted) return;
			console.log("Reconnecting to event stream");
		}
	}

	private async handleEvent(event: LobbyEvent): Promise<void> {
		switch (event.event.oneofKind) {
			case "info": {
				await this.handleInfoEvent(event.event.info);
				break;
			}
			case "playerJoin": {
				this.handlePlayerJoinEvent(event.event.playerJoin);
				break;
			}
			case "playerLeave": {
				this.handlePlayerLeaveEvent(event.event.playerLeave);
				break;
			}
			case "playerUpdate": {
				this.handlePlayerUpdateEvent(event.event.playerUpdate);
				break;
			}
			case "chatMessage": {
				this.handleChatMessageEvent(event.event.chatMessage);
				break;
			}
			case "stateUpdate": {
				this.handleStateUpdateEvent(event.event.stateUpdate);
				break;
			}
			case "countdown": {
				this.handleCountdownEvent(event.event.countdown);
				break;
			}
		}
	}

	private async handleInfoEvent(event: LobbyEventInfo): Promise<void> {
		const {
			players: eventPlayers,
			state: eventState,
			maxPlayers,
			passwordRequired,
			version,
			countdown,
			gamemode,
			enableJoinInProgress,
		} = event;
		players.value = eventPlayers;
		if (eventState) {
			state.value = eventState;
		}
		lobbyStaticInfo.value = {
			version,
			maxPlayers,
			gamemode,
			enableJoinInProgress,
		};
		if (countdown) {
			this.handleCountdownEvent(countdown);
		}
		if (eventPlayers.some((p) => p.id === playerId.value)) return;

		const hasValidInstance = Object.values(instanceMap.get()).some(
			(i) => i.version === version,
		);
		if (!hasValidInstance) {
			joinErrorCode.value = JoinLobbyClientErrorCode.NO_VALID_INSTANCE;
			return;
		}
		if (passwordRequired && !lobbyPassword.value) {
			joinErrorCode.value = JoinLobbyClientErrorCode.PASSWORD_REQUIRED;
			return;
		}
		if (eventPlayers.length >= maxPlayers) {
			joinErrorCode.value = JoinLobbyErrorCode.LOBBY_FULL;
			return;
		}

		await this.joinLobby();
	}

	private handlePlayerJoinEvent(event: LobbyEventPlayerJoin): void {
		const player = event.player;
		if (player) {
			players.value = [...players.value, player];
		}
	}

	private handlePlayerLeaveEvent(event: LobbyEventPlayerLeave): void {
		const playerIdValue = event.playerId;
		players.value = players.value.filter((p) => p.id !== playerIdValue);
	}

	private handlePlayerUpdateEvent(event: LobbyEventPlayerUpdate): void {
		const player = event.player;
		if (!player) return;
		players.value = players.value.map((p) => (p.id === player.id ? player : p));
	}

	private handleChatMessageEvent(event: LobbyEventChatMessage): void {
		const message = event.chatMessage;
		if (!message) return;
		const sender = players.value.find((p) => p.id === message.authorId);
		if (!sender || sender.taskForce !== ownTeam.value) return;
		chatMessages.value = [
			...chatMessages.value,
			{
				content: message.content,
				username: sender.displayName,
				sentAt: message.sentAt,
			},
		];
	}

	private handleStateUpdateEvent(event: LobbyEventStateUpdate): void {
		const eventState = event.state;
		console.log("State update received:", eventState);
		if (!eventState) return;
		state.value = eventState;
	}

	public getLaunchGameInstance(): Instance | null {
		const instance = Object.values(instanceMap.get()).find(
			(i) => i.version === lobbyStaticInfo.value?.version,
		);

		const player = players.value.find((p) => p.id === playerId.value);
		const isRunning = processesList.value.some((p) => p.instance.id === instance?.id);
		if (!player || isRunning || !player.champion || !instance) return null;

		const host = lobbyHost.value;
		let ip = "";
		try {
			ip = new URL(host).hostname;
		} catch {
			ip = host.replace(/^https?:\/\//, "").split(":")[0];
		}

		const gameServerPort = state.value.inGame?.gameServerPort ?? 7777;

		const name = username.value;
		const character = player.champion.toLowerCase();
		const team = player.taskForce;
		let arg = `${ip}:${gameServerPort}?name=${name}?class=${character}?team=${team}?horse=2`;
		if (lobbyPassword.value.length > 0) {
			arg += `?password=${lobbyPassword.value}`;
		}
		let existingArgs = instance.launchOptions.args;
		//the launch arguments could already contain an argument to join a server
		if (existingArgs.length > 0 && !existingArgs[0].startsWith("-")) {
			existingArgs = existingArgs.slice(1);
		}

		return {
			...instance,
			launchOptions: {
				...instance.launchOptions,
				args: [arg, ...existingArgs],
			},
		};
	}

	private handleCountdownEvent(event: LobbyEventCountdown): void {
		console.log(`Countdown: ${event.seconds} seconds`);
		this.countdown = event.seconds === 0 ? undefined : event;
	}

	async sendChatMessage(content: string): Promise<void> {
		if (!content.trim()) return;
		await this.getClient().sendChatMessage({ content }).response;
	}

	async selectChampion(championName: string): Promise<void> {
		await this.getClient().championSelect({ name: championName }).response;
	}

	async voteForMap(mapId: string): Promise<void> {
		console.log("Voting for map:", mapId, "with ticket:", ticket.value);
		try {
			const response = await this.getClient().mapVote({ mapId }).response;
			console.log("Vote response:", response);
		} catch (error) {
			console.error("Error voting for map:", error);
		}
	}

	async joinLobby(): Promise<void> {
		joinErrorCode.value = null;
		const joinResp = await this.getClient().joinLobby({
			authMethod: AuthMethod.PLAIN,
			authValue: username.value,
			password: lobbyPassword.value,
		});
		if (joinResp.response.result.oneofKind === "success") {
			ticket.value = joinResp.response.result.success.ticket;
			playerId.value = joinResp.response.result.success.playerId;
		} else if (joinResp.response.result.oneofKind === "error") {
			joinErrorCode.value = joinResp.response.result.error.code;
		}
	}

	async leaveLobby(): Promise<void> {
		try {
			await this.getClient().leaveLobby({}).response;
		} catch (error) {
			console.error("Error leaving lobby", error);
		}
		this.disconnect();
		resetLobbyState();
	}

	isConnected(): boolean {
		return this.abortController !== null && !this.abortController.signal.aborted;
	}

	cleanup(): void {
		resetLobbyState();
	}
}

export const lobbyManager = new LobbyManager();
