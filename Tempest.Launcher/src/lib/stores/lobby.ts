import { LobbyState, Timestamp } from "$lib/rpc";
import { LobbyPlayer } from "$lib/rpc/lobby/lobby_player";
import { persistentAtom } from "@nanostores/persistent";
import { atom } from "nanostores";

export const playerId = persistentAtom<string>("lobbyPlayerId", crypto.randomUUID());

export const lobbyHost = atom<string>("");
export const lobbyPassword = atom<string>("");
export const ticket = atom<string>("");
export const playerStore = atom<LobbyPlayer[]>([]);
export const chatMessageStore = atom<{ username: string; content: string; sentAt?: Timestamp }[]>(
	[],
);
export const stateStore = atom<LobbyState>({});

//used only for debug during development
export const debugPlayersStore = atom<Map<string, string>>(new Map()); //player id to ticket
