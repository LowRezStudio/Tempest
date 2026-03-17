import { GrpcWebFetchTransport } from "@protobuf-ts/grpcweb-transport";
import { fetch } from "@tauri-apps/plugin-http";
import { LobbyClient } from "./rpc/lobby/lobby_service.client";
import { ServerListClient } from "./rpc/server_list/server_list_service.client";
import { ticket } from "./stores/lobby";
import { servicesURL } from "./stores/settings";

let serverListConnectionCache: { host: string; client: ServerListClient } | null = null;
export function getConnectionToServerList() {
	const host = servicesURL.get();
	if (serverListConnectionCache !== null && serverListConnectionCache.host === host) {
		return serverListConnectionCache.client;
	}
	const transport = new GrpcWebFetchTransport({
		baseUrl: host,
		format: "binary",
		fetch,
	});

	const client = new ServerListClient(transport);
	serverListConnectionCache = { host, client };
	return client;
}

const connectionCache: Record<string, LobbyClient> = {};
export function getConnectionToServer(host: string) {
	if (host in connectionCache) {
		return connectionCache[host];
	}
	const transport = new GrpcWebFetchTransport({
		baseUrl: host,
		format: "binary",
		fetch,
		interceptors: [
			{
				interceptUnary(next, method, input, options) {
					if (!options.meta) {
						options.meta = {};
					}
					const ticketValue = ticket.get();
					if (ticketValue) {
						console.log("Adding x-ticket to request:", method.name, ticketValue);
						options.meta["x-ticket"] = ticketValue;
					} else {
						console.warn("No ticket available for request:", method.name);
					}

					return next(method, input, options);
				},
			},
		],
	});
	const client = new LobbyClient(transport);
	connectionCache[host] = client;
	return client;
}
export * from "./rpc/common/country";

export * from "./rpc/google/protobuf/timestamp";

export * from "./rpc/lobby/lobby_event";
export * from "./rpc/lobby/lobby_state";
export * from "./rpc/lobby/lobby_state_champion_select";
export * from "./rpc/lobby/lobby_state_game_in_progress";
export * from "./rpc/lobby/lobby_state_in_game";
export * from "./rpc/lobby/lobby_state_map_vote";
export * from "./rpc/lobby/lobby_state_waiting";

export * from "./rpc/server_list/server_listing";
