import { atom } from "nanostores";

export {
	playerId,
	lobbyHost,
	lobbyPassword,
	ticket,
	players as playerStore,
	chatMessages as chatMessageStore,
	state as stateStore,
} from "$lib/lobby/stores";

export const debugPlayersStore = atom<Map<string, string>>(new Map());
