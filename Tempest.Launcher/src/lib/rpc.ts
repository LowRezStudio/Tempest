import { GrpcWebFetchTransport } from "@protobuf-ts/grpcweb-transport";
import { fetch } from "@tauri-apps/plugin-http";
import { ServerListClient } from "./rpc/server_list/server_list_service.client";

const transport = new GrpcWebFetchTransport({
	baseUrl: "http://localhost:5197",
	format: "binary",
	fetch,
});

export const serverList = new ServerListClient(transport);

export * from "./rpc/lobby/lobby_event";
export * from "./rpc/lobby/lobby_info";
export * from "./rpc/server_list/server_listing";
