import { createCommand } from "./command";
import type { LobbyServerOptions } from "$lib/types/lobby";

export const hostLobby = async (options: LobbyServerOptions) => {
	const command = createCommand([
		"server",
		"open",
		...Object.entries(options)
			.filter(([, value]) => {
				return !!value;
			})
			.map(([key, value]) => {
				if (value === true) return `--${key}`;
				return [`--${key}`, value];
			}),
	]);
	//for debug
	command.stdout.on("data", (line) => {
		console.log("stdout:", line);
	});

	command.stderr.on("data", (line) => {
		console.error("stderr:", line);
	});
	//for now just spawning the child process
	//TODO do something smart
	const child = await command.spawn();
	console.log(child.pid);
};
