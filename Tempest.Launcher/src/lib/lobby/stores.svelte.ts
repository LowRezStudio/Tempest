import { Timestamp } from "$lib/rpc";
import { persistedState } from "$lib/stores/persisted.svelte";
import { getMapsForVersion } from "$lib/utils/versions";
import type { LobbyState } from "$lib/rpc";
import type { LobbyPlayer } from "$lib/rpc/lobby/lobby_player";
import type { Instance } from "$lib/types/instance";
import type { ExtendedJoinLobbyErrorCode, Map as LobbyMap } from "$lib/types/lobby";

export type ConnectionStatus = "connected" | "disconnected" | "pending";

export interface ChatMessage {
	username: string;
	content: string;
	channel: string;
	sentAt?: Timestamp;
}
export interface LobbyStaticInfo {
	version: string;
	maxPlayers: number;
	gamemode: string;
	enableJoinInProgress: boolean;
}

export interface LobbyWaitingState {
	isGameInProgress: boolean;
	isWaiting: boolean;
	isPendingConnection: boolean;
	isGameServerLaunching: boolean;
	isLobbyRestarting: boolean;
	canRejoinGame: boolean;
	canRejoinLobby: boolean;
	canJoinInProgress: boolean;
	playerCount: number;
	minimumPlayerCount: number;
	countdownSeconds: number;
	gameVersion: string;
	currentMap?: LobbyMap;
}

export const playerId = persistedState<string>(
	"lobbyPlayerId",
	typeof crypto !== "undefined" && crypto.randomUUID ? crypto.randomUUID() : "",
);

export const lobbyHost = persistedState<string>("lobbyConnection", "");
export const lobbyPassword = persistedState<string>("lobbyPassword", "");
export const ticket = persistedState<string>("lobbyTicket", "");
export const mostRecentLobbyConnectionTime = persistedState<string>(
	"lobbyMostRecentConnectionTime",
	"",
);

export const currentInstance = $state({ value: null as Instance | null });
export const players = $state({ value: [] as LobbyPlayer[] });
export const chatMessages = $state({ value: [] as ChatMessage[] });
export const state = $state({ value: {} as LobbyState });
export const connectionStatus = $state({ value: "pending" as ConnectionStatus });
export const joinErrorCode = $state({ value: null as ExtendedJoinLobbyErrorCode | null });

export const currentCountdownSeconds = $state({ value: -1 });

export const lobbyStaticInfo = $state({ value: null as LobbyStaticInfo | null });

export const debugPlayersStore = $state({ value: new Map<string, string>() });

export const ownTeam = {
	get value() {
		return players.value.find((p) => p.id === playerId.value)?.taskForce;
	},
};

export const teamLeft = {
	get value() {
		return players.value.filter((p) => p.taskForce === (ownTeam.value ?? 1));
	},
};

export const teamRight = {
	get value() {
		return players.value.filter((p) => p.taskForce !== (ownTeam.value ?? 1));
	},
};

export const isInChampionSelect = {
	get value() {
		return !!state.value.championSelect;
	},
};
export const isInMapVote = {
	get value() {
		return !!state.value.mapVote;
	},
};
export const isWaiting = {
	get value() {
		return !!state.value.waiting;
	},
};
export const isInGame = {
	get value() {
		return !!state.value.inGame;
	},
};
export const isGameServerOpen = {
	get value() {
		return !!state.value.inGame?.gameServerOpen;
	},
};

export const ownChampion = {
	get value() {
		return players.value.find((p) => p.id === playerId.value)?.champion ?? "";
	},
};

const FIVE_HOURS_MS = 5 * 60 * 60 * 1000;

export function clearStaleConnectionIfNeeded(): void {
	const timeStr = mostRecentLobbyConnectionTime.value;
	if (!timeStr) return;
	if (Date.now() - new Date(timeStr).getTime() > FIVE_HOURS_MS) {
		lobbyHost.value = "";
		lobbyPassword.value = "";
		ticket.value = "";
		mostRecentLobbyConnectionTime.value = "";
	}
}

export function resetLobbyState(): void {
	lobbyHost.value = "";
	lobbyPassword.value = "";
	ticket.value = "";
	mostRecentLobbyConnectionTime.value = "";
	players.value = [];
	chatMessages.value = [];
	state.value = {};
	connectionStatus.value = "pending";
	joinErrorCode.value = null;
	lobbyStaticInfo.value = null;
	currentInstance.value = null;
	currentCountdownSeconds.value = -1;
}

export const lobbyWaitingState = {
	get value() {
		const version = lobbyStaticInfo.value?.version ?? "0.57"; // TODO: Remove 0.57 placeholder
		const currentMap = lobbyStaticInfo.value?.version
			? getMapsForVersion(lobbyStaticInfo.value.version).find(
					(m) =>
						m.id === state.value.championSelect?.mapId ||
						m.id === state.value.inGame?.mapId,
				)
			: undefined;

		return {
			isGameInProgress: isInGame.value,
			isWaiting: isWaiting.value,
			isPendingConnection: connectionStatus.value === "pending",
			isGameServerLaunching: !isGameServerOpen.value && !state.value.inGame?.gameServerError,
			isLobbyRestarting: !!state.value.inGame?.gameServerFinishedRunning,
			canRejoinGame: !!(isGameServerOpen.value && ownChampion.value),
			canRejoinLobby:
				!players.value.some((p) => p.id === playerId.value) &&
				players.value.length < (lobbyStaticInfo.value?.maxPlayers ?? 0),
			canJoinInProgress: !!lobbyStaticInfo.value?.enableJoinInProgress && !ownChampion.value,
			playerCount: players.value.length,
			minimumPlayerCount: state.value.waiting?.minPlayers ?? 0,
			countdownSeconds: currentCountdownSeconds.value,
			gameVersion: version,
			currentMap,
		} satisfies LobbyWaitingState;
	},
};
