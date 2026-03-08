import { createMutation } from "@tanstack/svelte-query";
import { hostLobby } from "$lib/core/lobby";
import { lobbyManager } from "$lib/lobby/lobby-manager";
import type { LobbyServerOptions } from "$lib/types/lobby";

export const createLaunchLobbyMutation = () =>
	createMutation(() => ({
		mutationFn: (options: LobbyServerOptions) => hostLobby(options),
	}));

export const createLeaveLobbyMutation = () =>
	createMutation(() => ({
		mutationFn: () => lobbyManager.leaveLobby(),
	}));

export const createSendChatMessageMutation = () =>
	createMutation(() => ({
		mutationFn: (content: string) => lobbyManager.sendChatMessage(content),
	}));

export const createChampionSelectMutation = () =>
	createMutation(() => ({
		mutationFn: (championName: string) => lobbyManager.selectChampion(championName),
	}));

export const createMapSelectMutation = () =>
	createMutation(() => ({
		mutationFn: (mapId: string) => lobbyManager.voteForMap(mapId),
	}));
