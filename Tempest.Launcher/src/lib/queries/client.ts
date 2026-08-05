import type { QueryClient } from "@tanstack/svelte-query";

let queryClient: QueryClient | undefined;

export function setQueryClient(client: QueryClient): void {
	queryClient = client;
}

export function getQueryClient(): QueryClient | undefined {
	return queryClient;
}
