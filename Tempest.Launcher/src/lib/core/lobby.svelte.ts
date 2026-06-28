import { goto } from "$app/navigation";
import { lobbyManager } from "$lib/lobby/lobby-manager";
import { lobbyHost, resetLobbyState } from "$lib/lobby/stores.svelte";
import { appendProcessLog, lobbyServerProcessesList } from "$lib/stores/processes.svelte";
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
		process.returnCode.value = e.code;
	});

	let logsState = $state<ProcessLog[]>([]);
	let returnCodeState = $state<number | null>(null);

	const process: LobbyServerProcess = {
		createOptions: options,
		child,
		command,
		logs: {
			get value() {
				return logsState;
			},
			set value(v) {
				logsState = v;
			},
		},
		returnCode: {
			get value() {
				return returnCodeState;
			},
			set value(v) {
				returnCodeState = v;
			},
		},
	};
	let nextId = 0;
	function addLog(line: string, error: boolean) {
		process.logs.value = [...process.logs.value, { id: nextId++, line, error }].slice(-200);
		appendProcessLog(line, error, "lobby");
	}

	lobbyServerProcessesList.value = [...lobbyServerProcessesList.value, process];
	return child.pid;
};

export const killLobby = async (process: LobbyServerProcess) => {
	await process.child.write("kill\n");
};

export const moveToLobby = (host: string) => {
	if (lobbyHost.value === host) {
		void goto("/lobby");
		return;
	}
	if (lobbyManager.isConnected()) {
		lobbyManager.disconnect();
	}
	resetLobbyState();
	lobbyHost.value = host;
	void goto("/lobby");
};
