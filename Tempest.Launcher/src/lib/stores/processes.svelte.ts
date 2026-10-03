import { Command } from "@tauri-apps/plugin-shell";
import type { LobbyServerProcess, Process, ProcessLog } from "$lib/types/process";

export const processesList = $state({ value: [] as Process[] });

export const lobbyServerProcessesList = $state({ value: [] as LobbyServerProcess[] });

const MAX_LOGS = 5000;
const LOG_FLUSH_DELAY_MS = 50;
let nextLogId = 0;
let pendingLogs: ProcessLog[] = [];
let flushTimer: ReturnType<typeof setTimeout> | undefined;

export const processLogs = $state({ value: [] as ProcessLog[] });

function flushProcessLogs(): void {
	if (flushTimer !== undefined) {
		clearTimeout(flushTimer);
		flushTimer = undefined;
	}
	if (pendingLogs.length === 0) return;

	processLogs.value = [...processLogs.value, ...pendingLogs].slice(-MAX_LOGS);
	pendingLogs = [];
}

export function appendProcessLog(line: string, error = false, source = ""): void {
	appendProcessLogs([line], error, source);
}

export function appendProcessLogs(lines: string[], error = false, source = ""): void {
	if (lines.length === 0) return;
	const entries = lines.map((line) => ({ id: nextLogId++, line, error, source }));
	pendingLogs.push(...entries);
	if (pendingLogs.length > MAX_LOGS) pendingLogs = pendingLogs.slice(-MAX_LOGS);
	flushTimer ??= setTimeout(flushProcessLogs, LOG_FLUSH_DELAY_MS);
}

export function clearProcessLogs(): void {
	if (flushTimer !== undefined) clearTimeout(flushTimer);
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
	// Tauri emits one newline-stripped line per event; Electron forwards raw stream chunks.
	const receivesOutputChunks = typeof window !== "undefined" && window.electronAPI !== undefined;

	command.stdout.on("data", (data) => {
		if (receivesOutputChunks) {
			stdoutBuffer = appendLines(stdoutBuffer, data, source, false);
		} else {
			appendProcessLog(data, false, source);
		}
	});

	command.stderr.on("data", (data) => {
		if (receivesOutputChunks) {
			stderrBuffer = appendLines(stderrBuffer, data, source, true);
		} else {
			appendProcessLog(data, true, source);
		}
	});

	command.on("close", () => {
		if (stdoutBuffer) appendProcessLog(stdoutBuffer, false, source);
		if (stderrBuffer) appendProcessLog(stderrBuffer, true, source);
		flushProcessLogs();
	});
}
