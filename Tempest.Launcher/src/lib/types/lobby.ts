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
