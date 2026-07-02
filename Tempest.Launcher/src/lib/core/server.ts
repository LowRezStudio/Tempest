import { logCommandOutput } from "../stores/processes.svelte";
import { createCommand } from "./command";

export function createDiscoverCommand(timeoutMs = 1500) {
	const command = createCommand(["server", "discover", { "--timeout-ms": String(timeoutMs) }]);
	logCommandOutput(command, "lan-discovery");
	return command;
}
