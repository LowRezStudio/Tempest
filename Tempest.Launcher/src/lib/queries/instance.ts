import { createQuery } from "@tanstack/svelte-query";
import { path } from "@tauri-apps/api";
import { detectAvailablePlatforms } from "$lib/platforms/detection";
import type { InstancePlatform } from "$lib/types/instance";

export const createInstancePlatformsQuery = (instancePath: () => string) =>
	createQuery(() => {
		const pathValue = instancePath();
		return {
			queryKey: ["instance-platforms", pathValue],
			queryFn: (): Promise<InstancePlatform[]> =>
				detectAvailablePlatforms(pathValue),
			enabled: !!pathValue,
		};
	});

export const createDefaultInstancePathQuery = (
	selectedVersion: () => string | undefined,
	defaultPath: () => string | null | undefined,
) =>
	createQuery(() => {
		const version = selectedVersion();
		const basePath = defaultPath();
		return {
			queryKey: ["default-instance-path", basePath ?? "", version ?? ""],
			queryFn: async () => {
				if (!version) return basePath ?? "";
				if (basePath) {
					return await path.join(basePath, version);
				}
				return `/instances/${version}`;
			},
		};
	});
