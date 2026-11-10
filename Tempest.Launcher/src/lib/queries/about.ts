import { createQuery } from "@tanstack/svelte-query";
import { arch, getVersion, invoke, type as osType } from "$lib/platform/api";

export type AboutInfo = {
	appVersion: string;
	osName: string;
	architecture: string;
	buildDate: string;
};

export const createAboutInfoQuery = () =>
	createQuery(() => ({
		queryKey: ["about-info"],
		queryFn: async (): Promise<AboutInfo> => ({
			appVersion: await getVersion(),
			osName: osType(),
			architecture: arch(),
			buildDate: new Date(__BUILD_DATE__).toUTCString(),
		}),
	}));

export const createClientIdQuery = () =>
	createQuery(() => ({
		queryKey: ["client-id"],
		queryFn: () => invoke("app:client-id"),
		staleTime: Infinity,
		retry: false,
	}));
