import type { Instance } from "./instance";
import type { LobbyServerOptions } from "./lobby";
import type { Child, Command } from "@tauri-apps/plugin-shell";
import type { WritableAtom } from "nanostores";

export type Process = {
	status: "on" | "setup" | "off";
	child: Child;
	command: Command<string>;
	instance: Instance;
};

export type ProcessLog = {
	id: number;
	line: string;
	error: boolean;
};
export type LobbyServerProcess = {
	createOptions: LobbyServerOptions;
	child: Child;
	command: Command<string>;
	logs: WritableAtom<ProcessLog[]>;
	returnCode: WritableAtom<number | null>;
};
