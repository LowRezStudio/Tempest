import { JoinLobbyErrorCode } from "$lib/rpc/lobby/join_lobby_error_code";

export type LobbyServerOptions = {
	path: string;
	name: string;
	tags: string;
	map?: string;
	version: string;
	"max-players": string;
	"min-players": string;
	"public-server": boolean;
	gamemode: string | null;
	port: string;
	password?: string;
	"services-url"?: string;
	"no-default-args"?: boolean;
	platform?: string;
	game?: string;
	dll?: string[];
};
export enum JoinLobbyClientErrorCode {
	PASSWORD_REQUIRED = 100,
	NO_VALID_INSTANCE = 101,
}
export type ExtendedJoinLobbyErrorCode = JoinLobbyErrorCode | JoinLobbyClientErrorCode;

export type Map = {
	displayName: string;
	id: string;
	mode: string;
	iconPath: string;
};
