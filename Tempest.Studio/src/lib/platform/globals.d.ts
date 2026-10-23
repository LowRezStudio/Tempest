// Ambient renderer globals. The electronAPI bridge is fully typed against the
// host's IPC contract (electron/src/ipc/contract.ts).
import type { OsInfo, PreloadApi } from "../../../electron/src/ipc/contract";

declare global {
	interface Window {
		electronAPI?: PreloadApi;
		__os?: OsInfo;
	}
}

export {};
