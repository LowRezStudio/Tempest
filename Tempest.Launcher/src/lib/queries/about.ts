import { createQuery } from "@tanstack/svelte-query";
import { getVersion } from "@tauri-apps/api/app";
import { arch, type as osType } from "@tauri-apps/plugin-os";

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
