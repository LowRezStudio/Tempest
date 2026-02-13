import { createCommand } from "./command";

export const getVersion = () =>
	createCommand(["--version"])
		.execute()
		.then((res) => res.stdout);

export type BuildInfo = {
	Id: string;
	VersionGroup: string;
	PatchHotfix: boolean;
	PatchName: string;
	PatchWikiReference: string;
};

export const identifyBuild = async (path: string): Promise<BuildInfo | null> => {
	const res = await createCommand(["build", "identify", path, "--json"]).execute();
	if (res.code !== 0) return null;
	try {
		return JSON.parse(res.stdout);
	} catch (error) {
		console.error("Failed to parse build info:", error);
		return null;
	}
};
