import { goto } from "$app/navigation";
import { lobbyManager } from "$lib/lobby/lobby-manager";
import { lobbyHost, resetLobbyState } from "$lib/lobby/stores";
import { appendProcessLog, lobbyServerProcessesList } from "$lib/stores/processes";
import { atom } from "nanostores";
import { createCommand } from "./command";
import type { LobbyServerOptions } from "$lib/types/lobby";
import type { LobbyServerProcess, ProcessLog } from "$lib/types/process";

export const hostLobby = async (options: LobbyServerOptions) => {
	const command = createCommand([
		"server",
		"open",
		...Object.entries(options)
			.filter(([, value]) => {
				if (Array.isArray(value) && value.length === 0) return false;
				return !!value;
			})
			.map(([key, value]) => {
				if (value === true) return `--${key}`;
				if (Array.isArray(value)) return [`--${key}`, value.join(",")];
				return [`--${key}`, value];
			}),
	]);
	let stdoutBuffer = "";
	let stderrBuffer = "";

	function appendLines(buffer: string, data: string, error: boolean): string {
		const combined = buffer + data;
		const lines = combined.split("\n");
		const leftover = lines.pop() ?? "";
		const complete = lines.filter((line) => line || error);
		for (const line of complete) {
			addLog(line, error);
		}
		return leftover;
	}

	command.stdout.on("data", (data) => {
		stdoutBuffer = appendLines(stdoutBuffer, data, false);
	});

	command.stderr.on("data", (data) => {
		stderrBuffer = appendLines(stderrBuffer, data, true);
	});
	const child = await command.spawn();

	command.on("close", (e) => {
		if (stdoutBuffer) addLog(stdoutBuffer, false);
		if (stderrBuffer) addLog(stderrBuffer, true);
		process.returnCode.set(e.code);
	});

	const process: LobbyServerProcess = {
		createOptions: options,
		child,
		command,
		logs: atom<ProcessLog[]>([]),
		returnCode: atom<number | null>(null),
	};
	let nextId = 0;
	function addLog(line: string, error: boolean) {
		process.logs.set([...process.logs.get(), { id: nextId++, line, error }].slice(-200));
		appendProcessLog(line, error, "lobby");
	}

	lobbyServerProcessesList.set([...lobbyServerProcessesList.get(), process]);
	return child.pid;
};

export const killLobby = async (process: LobbyServerProcess) => {
	await process.child.write("kill\n");
};

export const moveToLobby = (host: string) => {
	if (lobbyHost.get() === host) {
		goto("/lobby");
		return;
	}
	if (lobbyManager.isConnected()) {
		lobbyManager.disconnect();
	}
	resetLobbyState();
	lobbyHost.set(host);
	goto("/lobby");
};
