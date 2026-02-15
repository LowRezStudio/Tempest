export type LobbyServerOptions = {
    name: string;
    tags: string;
    map?: string;
    version: string;
    "max-players": string;
    "public-server": boolean;
    gamemode: string | null;
    port: string;
    password?: string;
}