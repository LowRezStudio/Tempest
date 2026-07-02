import { createQuery } from "@tanstack/svelte-query";
import { createDiscoverCommand } from "$lib/core/server";
import { getConnectionToServerList } from "$lib/rpc";
import type { ServerListing } from "$lib/rpc";

async function fetchServers(): Promise<ServerListing[]> {
	try {
		const client = getConnectionToServerList();
		const servers: ServerListing[] = [];
		const call = client.getServers({});
		for await (const resp of call.responses) {
			servers.push(resp);
		}
		return servers;
	} catch (error) {
		console.error("Failed to fetch servers", error);
		throw error;
	}
}

async function fetchLanServers(): Promise<ServerListing[]> {
	const command = createDiscoverCommand();
	const seen = new Set<string>();
	const servers: ServerListing[] = [];

	let stdout = "";
	let child: Awaited<ReturnType<typeof command.spawn>> | null = null;

	try {
		child = await command.spawn();

		command.stdout.on("data", (dataRaw) => {
			const data = dataRaw as string;
			stdout += data;
			const lines = stdout.split("\n");
			stdout = lines.pop() ?? "";

			for (const line of lines) {
				const trimmed = line.trim();
				if (!trimmed) continue;
				try {
					const server = JSON.parse(trimmed) as ServerListing;
					if (!seen.has(server.id)) {
						seen.add(server.id);
						servers.push(server);
					}
				} catch (error) {
					console.warn("Failed to parse LAN server JSON:", trimmed, error);
				}
			}
		});

		await new Promise<void>((resolve, reject) => {
			command.on("close", () => resolve());
			command.on("error", (err) => reject(err));
		});
	} catch (error) {
		console.error("LAN discovery error:", error);
		throw error;
	} finally {
		child?.kill().catch(() => {});
	}

	return servers;
}

export function createServersQuery() {
	return createQuery(() => ({
		queryKey: ["servers"],
		queryFn: fetchServers,
		refetchInterval: 60000,
	}));
}

export function createLanServersQuery() {
	return createQuery(() => ({
		queryKey: ["lan-servers"],
		queryFn: fetchLanServers,
		enabled: false,
	}));
}
