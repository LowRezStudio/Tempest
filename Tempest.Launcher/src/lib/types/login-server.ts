import type { LoginServerDiscovery } from "$ipc/contract";

/** A login server saved for selection by any game instance. */
export type LoginServer = LoginServerDiscovery & {
	id: string;
	name: string;
};
