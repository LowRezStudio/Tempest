import type { Instance } from "../types/instance";
import type { Process } from "../types/process";
import { createCommand, processArgs } from "./command";
import { lastLaunchedInstanceId } from "../stores/instance";
import { processesList } from "../stores/processes";

export const launchGame = async (instance: Instance) => {
	const { path, launchOptions: options } = instance;
	const platform = options.platform ?? "Win64";

	lastLaunchedInstanceId.set(instance.id);

	const command = createCommand([
		"launch",
		path,
		{ "--platform": platform },
		{ "--no-default-args": options.noDefaultArgs },
		...(options.dllList ? options.dllList.map((dll) => ({ "--dll": dll })) : []),
		...(options.args ? ["--", ...processArgs(options.args)] : []),
	]);

	command.on("close", () => {
		processesList.set(processesList.get().filter((p) => p.instance.id !== instance.id));
	});

	const child = await command.spawn();

	const process: Process = {
		status: "on",
		child,
		command,
		instance,
	};

	processesList.set([...processesList.get(), process]);
};

export const killGame = async (instance: Instance) => {
	const processes = processesList.get();
	const process = processes.find((p) => p.instance.id === instance.id);

	if (process) await process.child.write("kill\n");
};
