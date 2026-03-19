import { createMutation } from "@tanstack/svelte-query";
import { hostLobby, killLobby } from "$lib/core/lobby";
import type { LobbyServerOptions } from "$lib/types/lobby";
import type { LobbyServerProcess } from "$lib/types/process";

export const createLaunchLobbyMutation = () =>
	createMutation(() => ({
		mutationFn: (options: LobbyServerOptions) => hostLobby(options),
	}));

export const createKillLobbyServerMutation = () =>
	createMutation(() => ({
		mutationFn: (process: LobbyServerProcess) => killLobby(process),
	}));
