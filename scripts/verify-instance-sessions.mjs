import assert from "node:assert/strict";
import { readFile, writeFile, mkdir, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const launcher = resolve(root, "Tempest.Launcher");
const require = createRequire(resolve(launcher, "package.json"));
const ts = require("typescript");
const fixture = resolve(launcher, ".svelte-kit", `verify-sessions-${process.pid}.mjs`);
const source = await readFile(resolve(launcher, "src/lib/core/launch.ts"), "utf8");
const js = ts.transpileModule(source.replace(/^import[\s\S]*?;\r?\n/gm, ""), { compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext } }).outputText;
const make = (id) => ({ id, path: `C:/Games/${id}`, version: "8.1", state: { type: "prepared" }, launchOptions: { args: [], dllList: [] } });
const a = make("a"), b = make("b");
const processesList = { value: [] }, launchingInstanceIds = { value: [] };
const commands = [];
let pid = 100, closeBeforeSpawn = false, failedWritePid = 0;
const setting = { get: () => undefined };
globalThis.__sessionTest = {
	instanceMap: { value: { a, b } }, lastLaunchedInstanceId: { value: undefined }, processesList, launchingInstanceIds,
	appendProcessLog: () => {}, logCommandOutput: () => {}, processArgs: (args) => args,
	gamescopeArgs: setting, protonPath: setting, useGamescope: setting, useSteamRuntime: setting, winePath: setting, wineRuntime: setting,
	createCommand: () => {
		const callbacks = {}, writes = [];
		const child = { pid: ++pid, write: async (data) => { if (child.pid === failedWritePid) throw new Error("pipe closed"); writes.push(data); } };
		const command = { on: (name, callback) => { callbacks[name] = callback; }, spawn: async () => { if (closeBeforeSpawn) callbacks.close(); return child; }, close: () => callbacks.close(), child, writes };
		commands.push(command);
		return command;
	},
};
try {
	await mkdir(dirname(fixture), { recursive: true });
	await writeFile(fixture, `const {instanceMap,lastLaunchedInstanceId,appendProcessLog,launchingInstanceIds,logCommandOutput,processesList,gamescopeArgs,protonPath,useGamescope,useSteamRuntime,winePath,wineRuntime,createCommand,processArgs}=globalThis.__sessionTest;\n` + js);
	const { launchGame, killGame, stopGameSession } = await import(pathToFileURL(fixture));
	await launchGame(a); await launchGame(a); await launchGame(b);
	assert.equal(processesList.value.length, 3);
	assert.deepEqual(processesList.value.map((p) => p.sessionNumber), [1, 2, 1]);
	assert.equal(new Set(processesList.value.map((p) => p.sessionId)).size, 3);
	await stopGameSession(processesList.value[0].sessionId);
	assert.deepEqual(commands.map((c) => c.writes.length), [1, 0, 0]);
	commands[0].close();
	assert.equal(processesList.value.length, 2);
	assert.equal(processesList.value[0].sessionNumber, 2);
	await killGame(a);
	assert.deepEqual(commands.map((c) => c.writes.length), [1, 1, 0]);
	commands[1].close();
	assert.equal(processesList.value[0].instance.id, "b");
	await launchGame(a); await launchGame(a);
	failedWritePid = commands[3].child.pid;
	await assert.rejects(killGame(a), /1 session/);
	assert.equal(processesList.value.find((p) => p.child.pid === failedWritePid).status, "on");
	assert.deepEqual(commands[4].writes, ["kill\n"]);
	await assert.rejects(launchGame(a), /finish stopping/);
	commands[3].close(); commands[4].close();
	closeBeforeSpawn = true;
	await launchGame(a);
	assert.equal(processesList.value.length, 1);
	assert.deepEqual(launchingInstanceIds.value, []);
	closeBeforeSpawn = false;
	const first = launchGame(a);
	await assert.rejects(launchGame(a), /finish launching/);
	await first;
	assert.equal(processesList.value.length, 2);
	const guardSource = await readFile(resolve(launcher, "src/lib/core/session-guards.ts"), "utf8");
	const guardJs = ts.transpileModule(guardSource.replace(/^import[\s\S]*?;\r?\n/gm, ""), { compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext } }).outputText;
	const guardPrefix = `const {instanceMap, processesList, launchingInstanceIds} = globalThis.__sessionTest; const platform = () => "windows";\n`;
	const { assertNoGameSessions } = await import(`data:text/javascript;base64,${Buffer.from(guardPrefix + guardJs).toString("base64")}`);
	assert.throws(() => assertNoGameSessions("c:\\games\\A\\Binaries\\Win64\\Paladins.exe"), /Close all sessions/);
	assertNoGameSessions("C:/Games/unrelated");
	processesList.value = [];
	launchingInstanceIds.value = ["b"];
	assert.throws(() => assertNoGameSessions("C:/Games/b"), /Close all sessions/);
	launchingInstanceIds.value = [];
	assertNoGameSessions("C:/Games/b");
	// Reconciliation drops one exited window while retaining another window of the same instance.
	processesList.value = [
		{ instance: a, sessionId: "a-1", child: { pid: 1 } },
		{ instance: a, sessionId: "a-2", child: { pid: 2 } },
		{ instance: b, sessionId: "b-1", child: { pid: 3 } },
	];
	const monitorSource = await readFile(resolve(launcher, "src/lib/core/process-monitor.ts"), "utf8");
	const monitorJs = ts.transpileModule(monitorSource.replace(/^import[\s\S]*?;\r?\n/gm, ""), { compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext } }).outputText;
	const monitorPrefix = `const {processesList} = globalThis.__sessionTest; const invoke = async (_, {pid}) => pid !== 1; const window = {setInterval: () => 1, clearInterval() {}, addEventListener() {}, removeEventListener() {}}; const document = {visibilityState: "visible", addEventListener() {}, removeEventListener() {}};\n`;
	const monitor = await import(`data:text/javascript;base64,${Buffer.from(monitorPrefix + monitorJs).toString("base64")}`);
	const stopMonitor = monitor.startProcessMonitor();
	await new Promise(setImmediate);
	assert.deepEqual(processesList.value.map((process) => process.sessionId), ["a-2", "b-1"]);
	stopMonitor();
	assert.deepEqual(launchingInstanceIds.value, []);
	console.log("Sessions: multiple launches, stop-one/stop-all scope, stable numbering, natural closes, partial stop failure, early close, concurrent-launch guards, file mutation guards and session reconciliation passed.");
} finally {
	delete globalThis.__sessionTest;
	await rm(fixture, { force: true });
}
