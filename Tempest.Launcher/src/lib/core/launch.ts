import { instanceMap, lastLaunchedInstanceId } from "../stores/instance.svelte";
import {
	appendProcessLog,
	launchingInstanceIds,
	logCommandOutput,
	processesList,
} from "../stores/processes.svelte";
import {
	gamescopeArgs,
	protonPath,
	useGamescope,
	useSteamRuntime,
	winePath,
	wineRuntime,
} from "../stores/settings.svelte";
import { createCommand, processArgs } from "./command";
import type { Instance } from "../types/instance";
import type { Process } from "../types/process";

const sessionNumbers = new Map<string, number>();

export const launchGame = async (requestedInstance: Instance) => {
	if (launchingInstanceIds.value.includes(requestedInstance.id)) {
		throw new Error("Wait for this instance to finish launching.");
	}
	if (
		processesList.value.some(
			(p) => p.instance.id === requestedInstance.id && p.status === "stopping",
		)
	) {
		throw new Error("Wait for this instance to finish stopping.");
	}
	launchingInstanceIds.value = [...launchingInstanceIds.value, requestedInstance.id];
	try {
		await startGameSession(requestedInstance);
	} finally {
		launchingInstanceIds.value = launchingInstanceIds.value.filter(
			(id) => id !== requestedInstance.id,
		);
	}
};

const startGameSession = async (requestedInstance: Instance) => {
	const instance = instanceMap.value[requestedInstance.id] ?? requestedInstance;
	if (instance.state.type !== "prepared") {
		throw new Error("Finish preparing this instance before launching it.");
	}
	const { path, launchOptions: options } = instance;
	const platform = options.platform ?? "Win64";

	lastLaunchedInstanceId.value = instance.id;

	console.log("Launching instance", instance.id, instance.version);
	console.log(instance);

	appendProcessLog(
		`Launching game client with args: ${processArgs(options.args ?? []).join(" ")}`,
		false,
		"launch",
	);

	const wine = winePath.get();
	const runtime = wineRuntime.get();
	const proton =
		runtime === "proton" ? protonPath.get() : runtime === "wine" ? "wine" : undefined;
	const env: Record<string, string> | undefined =
		wine || proton
			? { ...(wine ? { WINE: wine } : {}), ...(proton ? { PROTON: proton } : {}) }
			: undefined;

	const command = createCommand(
		[
			"launch",
			path,
			{ "--platform": platform },
			{ "--no-default-args": options.noDefaultArgs },
			{ "--gamescope": useGamescope.get() },
			{ "--gamescope-args": gamescopeArgs.get() || undefined },
			{ "--steam-runtime": useSteamRuntime.get() || undefined },
			{
				"--homedir":
					instance.version === "8.1"
						? "Paladins"
						: `${instance.version ? `${instance.version}_` : ""}${instance.label}`.replaceAll(
								/[^a-zA-Z0-9-_]/g,
								"_",
							),
			},
			...(options.dllList ? options.dllList.map((dll) => ({ "--dll": dll })) : []),
			...(options.args ? ["--", ...processArgs(options.args)] : []),
		],
		env,
	);

	logCommandOutput(command, "launch");

	const launchState = { childPid: undefined as number | undefined, closed: false };
	command.on("close", () => {
		launchState.closed = true;
		if (launchState.childPid !== undefined) {
			processesList.value = processesList.value.filter(
				(p) => p.child.pid !== launchState.childPid,
			);
		}
	});

	const child = await command.spawn();
	launchState.childPid = child.pid;
	if (launchState.closed) return;

	const sessionId = crypto.randomUUID();
	const sessionNumber = (sessionNumbers.get(instance.id) ?? 0) + 1;
	sessionNumbers.set(instance.id, sessionNumber);
	const process: Process = {
		status: "on",
		sessionId,
		sessionNumber,
		startedAt: Date.now(),
		child,
		command,
		instance,
	};

	processesList.value = [...processesList.value, process];
};

export const killGame = async (instance: Instance) => {
	if (launchingInstanceIds.value.includes(instance.id)) {
		throw new Error("Wait for this instance to finish launching.");
	}
	const sessions = processesList.value.filter((p) => p.instance.id === instance.id);
	const results = await Promise.allSettled(sessions.map((p) => stopGameSession(p.sessionId)));
	const errors = results.filter((result) => result.status === "rejected");
	if (errors.length) {
		throw new Error(
			`${errors.length} session(s) could not be stopped. Try closing them individually.`,
		);
	}
};

export const stopGameSession = async (sessionId: string) => {
	const process = processesList.value.find((p) => p.sessionId === sessionId);
	if (!process || process.status === "stopping") return;
	process.status = "stopping";
	try {
		await process.child.write("kill\n");
	} catch (error) {
		process.status = "on";
		throw error;
	}
};
