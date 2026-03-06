import { path as tauriPath } from "@tauri-apps/api";
import { appConfigDir } from "@tauri-apps/api/path";

export const getInstanceBasePath = async (id: string): Promise<string> => {
	const configDir = await appConfigDir();
	return tauriPath.join(configDir, "instances", id);
};

export const getInstanceTokensDir = async (id: string): Promise<string> => {
	const baseDir = await getInstanceBasePath(id);
	return tauriPath.join(baseDir, "tokens");
};

export const getFieldsTokenPath = async (id: string): Promise<string> => {
	const tokensDir = await getInstanceTokensDir(id);
	return tauriPath.join(tokensDir, "fields.dat");
};

export const getFunctionsTokenPath = async (id: string): Promise<string> => {
	const tokensDir = await getInstanceTokensDir(id);
	return tauriPath.join(tokensDir, "functions.dat");
};

export const getInstanceAssemblyDbPath = async (id: string): Promise<string> => {
	const baseDir = await getInstanceBasePath(id);
	return tauriPath.join(baseDir, "assembly.db");
};

export const getInstanceAssemblyDbConnectionString = (id: string): string =>
	`sqlite:instances/${id}/assembly.db`;
