import { createMutation } from "@tanstack/svelte-query";
import type { LobbyServerOptions } from "$lib/types/lobby";
import { hostLobby } from "$lib/core/lobby";
import type { LobbyClient } from "$lib/rpc/lobby/lobby_service.client";

export const createLaunchLobbyMutation = () =>
	createMutation(() => ({
		mutationFn: (options: LobbyServerOptions) => hostLobby(options),
	}));

export const createLeaveLobbyMutation = (client: LobbyClient) =>
	createMutation(() => ({
		mutationFn: () => client.leaveLobby({}).response,
	}));

export const createSendChatMessageMutation = (client: LobbyClient) =>
	createMutation(() => ({
		mutationFn: (chatboxText: string) =>
			client.sendChatMessage({
				content: chatboxText,
			}).response,
	}));

export const createChampionSelectMutation = (client: LobbyClient) =>
	createMutation(() => ({
		mutationFn: (championName: string) =>
			client.championSelect({
				name: championName,
			}).response,
	}));

export const createMapSelectMutation = (client: LobbyClient) =>
	createMutation(() => ({
		mutationFn: (mapId: string) =>
			client.mapVote({
				mapId: mapId,
			}).response,
	}));
