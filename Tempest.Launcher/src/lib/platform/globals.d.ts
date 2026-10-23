// Ambient renderer globals. The electronAPI bridge is fully typed against the
// host's IPC contract (electron/src/ipc/contract.ts), which is the single
// source of truth for channels, payloads and events.
import type { OsInfo, PreloadApi } from "../../../electron/src/ipc/contract";

declare global {
	interface Window {
		electronAPI?: PreloadApi;
		/** localStorage dump from the previous (Tauri) profile, injected by preload. */
		__migrationDump?: string | null;
		/** Renderer state parked in the host, injected by preload before page code. */
		__hostState?: Record<string, unknown>;
		/** OS info injected by preload, refined by the main process after load. */
		__os?: OsInfo;
	}
}

export {};
