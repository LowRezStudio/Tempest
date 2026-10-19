import { appConfigDir, path as sysPath } from "$lib/platform/api";

export const getInstanceBasePath = async (id: string): Promise<string> => {
	const configDir = await appConfigDir();
	return sysPath.join(configDir, "instances", id);
};

export const getInstanceTokensDir = async (id: string): Promise<string> => {
	const baseDir = await getInstanceBasePath(id);
	return sysPath.join(baseDir, "tokens");
};

export const getFieldsTokenPath = async (id: string): Promise<string> => {
	const tokensDir = await getInstanceTokensDir(id);
	return sysPath.join(tokensDir, "fields.dat");
};

export const getFunctionsTokenPath = async (id: string): Promise<string> => {
	const tokensDir = await getInstanceTokensDir(id);
	return sysPath.join(tokensDir, "functions.dat");
};

export const getInstanceAssemblyDbPath = async (id: string): Promise<string> => {
	const baseDir = await getInstanceBasePath(id);
	return sysPath.join(baseDir, "assembly.db");
};

export const getInstanceAssemblyDbConnectionString = (id: string): string =>
	`sqlite:instances/${id}/assembly.db`;
