import { createQuery } from "@tanstack/svelte-query";
import { serverList, type ServerListing } from "$lib/rpc";

async function fetchServers(): Promise<ServerListing[]> {
	try {
		const servers: ServerListing[] = [];
		const call = serverList.getServers({});
		for await (const resp of call.responses) {
			servers.push(resp);
		}
		return servers;
	} catch (error) {
		console.error("Failed to fetch servers", error);
		throw error;
	}
}

export function createServersQuery() {
	return createQuery(() => ({
		queryKey: ["servers"],
		queryFn: fetchServers,
		refetchInterval: 2000,
	}));
}
