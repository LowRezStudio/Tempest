import type { Child, Command } from "$lib/platform/api";
import type { Instance } from "./instance";
import type { LobbyServerOptions } from "./lobby";

export type Process = {
	status: "on" | "setup" | "off" | "stopping";
	/** Window number within its instance, stable while other windows close. */
	sessionNumber: number;
	startedAt: number;
	child: Child;
	command: Command<string>;
	instance: Instance;
};

export type ProcessLog = {
	id: number;
	line: string;
	error: boolean;
	source?: string;
};

export type LobbyServerProcess = {
	createOptions: LobbyServerOptions;
	child: Child;
	command: Command<string>;
	logs: { value: ProcessLog[] };
	returnCode: { value: number | null };
};
