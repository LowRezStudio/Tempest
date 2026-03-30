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

export const playerId = persistentAtom<string>("lobbyPlayerId", crypto.randomUUID());

export const lobbyHost = atom<string>("");
export const lobbyVersion = atom<string | null>(null);
export const lobbyMaxPlayers = atom<number | null>(null);
export const lobbyPassword = atom<string>("");
export const ticket = atom<string>("");
export const players = atom<LobbyPlayer[]>([]);
export const chatMessages = atom<ChatMessage[]>([]);
export const state = atom<LobbyState>({});
export const connectionStatus = atom<ConnectionStatus>("pending");
export const joinErrorCode = atom<ExtendedJoinLobbyErrorCode | null>(null);
export const currentInstance = atom<Instance | null>(null);
export const currentCountdownSeconds = atom<number>(-1);

export const debugPlayersStore = atom<Map<string, string>>(new Map());

export const ownTeam = computed(
	players,
	($players) => $players.find((p) => p.id === playerId.get())?.taskForce,
);

export const teamLeft = computed([players, ownTeam], ($players, $ownTeam) =>
	$players.filter((p) => p.taskForce === $ownTeam),
);

export const teamRight = computed([players, ownTeam], ($players, $ownTeam) =>
	$players.filter((p) => p.taskForce !== $ownTeam),
);

export const isInChampionSelect = computed(state, ($state) => !!$state.championSelect);
export const isInMapVote = computed(state, ($state) => !!$state.mapVote);
export const isWaiting = computed(state, ($state) => !!$state.waiting);
export const isInGame = computed(state, ($state) => !!$state.inGame);
export const isGameServerOpen = computed(state, ($state) => !!$state.inGame?.gameServerOpen);

export function resetLobbyState(): void {
	lobbyHost.set("");
	lobbyPassword.set("");
	ticket.set("");
	players.set([]);
	chatMessages.set([]);
	state.set({});
	connectionStatus.set("pending");
	joinErrorCode.set(null);
	lobbyVersion.set(null);
	lobbyMaxPlayers.set(null);
	currentInstance.set(null);
}
