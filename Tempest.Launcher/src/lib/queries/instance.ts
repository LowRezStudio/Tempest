import { createMutation, createQuery } from "@tanstack/svelte-query";
import { detectAvailablePlatforms } from "$lib/platforms/detection";
import { findPaladinsInstallations, type FoundInstallation } from "$lib/platforms/discovery";
import { setupInstance } from "$lib/platforms/setup";
import type { Instance, InstancePlatform } from "$lib/types/instance";

/**
 * The Paladins installs on this machine, kept for a minute so reopening the wizard is instant.
 * Each install joins the data as soon as it is found, while the scan still runs.
 * @param folders - Folders Tempest downloads into.
 * @param enabled - Whether to look yet.
 * @returns The query.
 */
export const createFoundInstallationsQuery = (folders: () => string[], enabled: () => boolean) =>
	createQuery(() => {
		const list = folders();
		return {
			queryKey: ["found-installations", list],
			queryFn: ({ client, queryKey }) =>
				findPaladinsInstallations(list, true, (install) =>
					client.setQueryData<FoundInstallation[]>(queryKey, (found = []) =>
						found.some((f) => f.path === install.path) ? found : [...found, install],
					),
				),
			enabled: enabled(),
			staleTime: 60_000,
			// A failed scan won't succeed on its own a second later; Look again retries.
			retry: false,
		};
	});

export const createInstancePlatformsQuery = (instancePath: () => string) =>
	createQuery(() => {
		const pathValue = instancePath();
		return {
			queryKey: ["instance-platforms", pathValue],
			queryFn: (): Promise<InstancePlatform[]> => detectAvailablePlatforms(pathValue),
			enabled: !!pathValue,
		};
	});

export const createSetupInstanceMutation = () =>
	createMutation(() => ({
		mutationFn: (instance: Instance): Promise<void> => setupInstance(instance),
	}));
