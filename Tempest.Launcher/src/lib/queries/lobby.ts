import { createMutation } from "@tanstack/svelte-query";
import type { LobbyServerOptions } from "$lib/types/lobby";
import { hostLobby } from "$lib/core/lobby";

export const createLaunchLobbyMutation = () =>
	createMutation(() => ({
		mutationFn: (options: LobbyServerOptions) => hostLobby(options),
	}));
