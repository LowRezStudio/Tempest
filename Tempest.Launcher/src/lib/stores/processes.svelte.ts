import { Command } from "@tauri-apps/plugin-shell";
import type { LobbyServerProcess, Process, ProcessLog } from "$lib/types/process";

export const processesList = $state({ value: [] as Process[] });

export const lobbyServerProcessesList = $state({ value: [] as LobbyServerProcess[] });

const MAX_LOGS = 5000;
let nextLogId = 0;

export const processLogs = $state({ value: [] as ProcessLog[] });

export function appendProcessLog(line: string, error = false, source = ""): void {
	appendProcessLogs([line], error, source);
}

// Chatty processes (e.g. Wine) can print hundreds of lines per second; copying the whole log for
// every line freezes the UI, so new lines are flushed in batches.
const LOG_FLUSH_MS = 50;
let pendingLogs: ProcessLog[] = [];
let flushTimer: ReturnType<typeof setTimeout> | undefined;

function flushProcessLogs(): void {
	flushTimer = undefined;
	processLogs.value = [...processLogs.value, ...pendingLogs].slice(-MAX_LOGS);
	pendingLogs = [];
}

export function appendProcessLogs(lines: string[], error = false, source = ""): void {
	if (lines.length === 0) return;
	for (const line of lines) pendingLogs.push({ id: nextLogId++, line, error, source });
	flushTimer ??= setTimeout(flushProcessLogs, LOG_FLUSH_MS);
}

export function clearProcessLogs(): void {
	clearTimeout(flushTimer);
	flushTimer = undefined;
	pendingLogs = [];
	processLogs.value = [];
}

function appendLines(buffer: string, data: string, source: string, error: boolean): string {
	const combined = buffer + data;
	const lines = combined.split("\n");
	const leftover = lines.pop() ?? "";
	const complete = lines.filter((line) => line || error);
	if (complete.length > 0) appendProcessLogs(complete, error, source);
	return leftover;
}

export function logCommandOutput(command: Command<string>, source: string): void {
	let stdoutBuffer = "";
	let stderrBuffer = "";

	command.stdout.on("data", (data) => {
		stdoutBuffer = appendLines(stdoutBuffer, data, source, false);
	});

	command.stderr.on("data", (data) => {
		stderrBuffer = appendLines(stderrBuffer, data, source, true);
	});

	command.on("close", () => {
		if (stdoutBuffer) appendProcessLog(stdoutBuffer, false, source);
		if (stderrBuffer) appendProcessLog(stderrBuffer, true, source);
	});
}
