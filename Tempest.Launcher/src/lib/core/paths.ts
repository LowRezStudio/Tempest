import { invoke } from "$lib/platform/api";
// Instance data paths. Computed host-side (`instance:paths`) so the layout
// (userData dir pinning, separators, etc.) lives entirely in the Electron
// main process; this is a single typed IPC call.
import type { InstancePaths } from "$ipc/contract";

export function getInstancePaths(id: string): Promise<InstancePaths> {
	return invoke("instance:paths", { id });
}

export const getInstanceBasePath = (id: string): Promise<string> =>
	getInstancePaths(id).then((paths) => paths.base);

export const getInstanceTokensDir = (id: string): Promise<string> =>
	getInstancePaths(id).then((paths) => paths.tokensDir);

export const getFieldsTokenPath = (id: string): Promise<string> =>
	getInstancePaths(id).then((paths) => paths.fieldsToken);

export const getFunctionsTokenPath = (id: string): Promise<string> =>
	getInstancePaths(id).then((paths) => paths.functionsToken);

export const getInstanceAssemblyDbPath = (id: string): Promise<string> =>
	getInstancePaths(id).then((paths) => paths.assemblyDb);

export const getInstanceAssemblyDbConnectionString = (id: string): string =>
	`sqlite:instances/${id}/assembly.db`;
