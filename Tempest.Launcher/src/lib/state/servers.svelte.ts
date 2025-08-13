import { Server, serverList } from "$lib/rpc";

let loaded = false;

export type SearchFilters = {
	name: string;
	// region: "any" | "eu" | "na" | "sa" | "me" | "as" | "oc";
	// sort: "players" | "alphabet";
};

export const list = $state<Server[]>([]);
export const searchFilters = $state<SearchFilters>({
	name: "",
	// region: "any",
	// sort: "players"
});

export const refresh = async () => {
	try {
		const serverStream = serverList.getServers({});

		list.length = 0;

		for await (const server of serverStream.responses) {
			list.push(server);
		}

		loaded = true;
	} catch (error) {
		console.error("Failed to fetch server list:", error);

		loaded = false;
	}
};

export const ensureLoaded = async () => {
	if (loaded) return;
	await refresh();
};
