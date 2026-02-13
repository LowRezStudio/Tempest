import { invoke } from "@tauri-apps/api/core";

export const allowScopeDirectory = (path: string, recursive = true): Promise<void> =>
	invoke("scopes_allow_directory", { path, recursive });

export const allowScopeFile = (path: string): Promise<void> =>
	invoke("scopes_allow_file", { path });

export const forbidScopeFile = (path: string): Promise<void> =>
	invoke("scopes_forbid_file", { path });
