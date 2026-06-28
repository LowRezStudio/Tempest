import { JoinLobbyErrorCode } from "$lib/rpc/lobby/join_lobby_error_code";

export const GAMEMODES = [
	{ id: "TempestMp.SiegeDEV", label: "Siege" },
	{ id: "TempestMp.Payload", label: "Payload" },
	{ id: "TempestMp.Tdm", label: "Team Deathmatch" },
	{ id: "TempestMp.Onslaught", label: "Onslaught" },
] as const;

export type GameModeId = (typeof GAMEMODES)[number]["id"];

export function resolveGamemodeLabel(gamemode: string | undefined | null): string {
	if (!gamemode) return "Game";
	const normalized = gamemode.toLowerCase();
	const found = GAMEMODES.find((gm) => gm.id.toLowerCase() === normalized);
	if (found) return found.label;

	// Handle short/raw names directly (e.g. "siege", "tdm")
	if (normalized === "siege") return "Siege";
	if (normalized === "payload") return "Payload";
	if (normalized === "tdm") return "Team Deathmatch";
	if (normalized === "onslaught") return "Onslaught";

	// Fallback to capitalizing raw codename
	return gamemode.charAt(0).toUpperCase() + gamemode.slice(1);
}

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
	"game-server-port": string;
	password?: string;
	"services-url"?: string;
	"no-default-args"?: boolean;
	platform?: string;
	game?: string;
	dll?: string[];
	"enable-join-in-progress": boolean;
	upnp: boolean;
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
