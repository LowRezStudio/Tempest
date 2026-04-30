import { persistentAtom } from "@nanostores/persistent";
import { Timestamp } from "$lib/rpc";
import { atom, computed } from "nanostores";
import type { LobbyState } from "$lib/rpc";
import type { LobbyPlayer } from "$lib/rpc/lobby/lobby_player";
import type { Instance } from "$lib/types/instance";
import type { ExtendedJoinLobbyErrorCode } from "$lib/types/lobby";

export type ConnectionStatus = "connected" | "disconnected" | "pending";

export interface ChatMessage {
	username: string;
	content: string;
	sentAt?: Timestamp;
}
export interface LobbyStaticInfo {
	version: string;
	maxPlayers: number;
	gamemode: string;
	enableJoinMidGame: boolean;
}

export const playerId = persistentAtom<string>("lobbyPlayerId", crypto.randomUUID());

export const lobbyHost = persistentAtom<string>("lobbyConnection", "");
export const lobbyPassword = persistentAtom<string>("lobbyPassword", "");
export const ticket = persistentAtom<string>("lobbyTicket", "");
export const mostRecentLobbyConnectionTime = persistentAtom<string>(
	"lobbyMostRecentConnectionTime",
	"",
);

export const currentInstance = atom<Instance | null>(null);
export const players = atom<LobbyPlayer[]>([]);
export const chatMessages = atom<ChatMessage[]>([]);
export const state = atom<LobbyState>({});
export const connectionStatus = atom<ConnectionStatus>("pending");
export const joinErrorCode = atom<ExtendedJoinLobbyErrorCode | null>(null);

export const currentCountdownSeconds = atom<number>(-1);

export const lobbyStaticInfo = atom<LobbyStaticInfo | null>(null);

export const debugPlayersStore = atom<Map<string, string>>(new Map());

export const ownTeam = computed(
	players,
	($players) => $players.find((p) => p.id === playerId.get())?.taskForce,
);

export const teamLeft = computed([players, ownTeam], ($players, $ownTeam) =>
	$players.filter((p) => p.taskForce === ($ownTeam || 1)),
);

export const teamRight = computed([players, ownTeam], ($players, $ownTeam) =>
	$players.filter((p) => p.taskForce !== ($ownTeam || 1)),
);

export const isInChampionSelect = computed(state, ($state) => !!$state.championSelect);
export const isInMapVote = computed(state, ($state) => !!$state.mapVote);
export const isWaiting = computed(state, ($state) => !!$state.waiting);
export const isInGame = computed(state, ($state) => !!$state.inGame);
export const isGameServerOpen = computed(state, ($state) => !!$state.inGame?.gameServerOpen);

export const ownChampion = computed(
	[players, playerId],
	($players, $playerId) => $players.find((p) => p.id == $playerId)?.champion || "",
);

const FIVE_HOURS_MS = 5 * 60 * 60 * 1000;

export function clearStaleConnectionIfNeeded(): void {
	const timeStr = mostRecentLobbyConnectionTime.get();
	if (!timeStr) return;
	if (Date.now() - new Date(timeStr).getTime() > FIVE_HOURS_MS) {
		lobbyHost.set("");
		lobbyPassword.set("");
		ticket.set("");
		mostRecentLobbyConnectionTime.set("");
	}
}

export function resetLobbyState(): void {
	lobbyHost.set("");
	lobbyPassword.set("");
	ticket.set("");
	mostRecentLobbyConnectionTime.set("");
	players.set([]);
	chatMessages.set([]);
	state.set({});
	connectionStatus.set("pending");
	joinErrorCode.set(null);
	lobbyStaticInfo.set(null);
	currentInstance.set(null);
	currentCountdownSeconds.set(-1);
}
