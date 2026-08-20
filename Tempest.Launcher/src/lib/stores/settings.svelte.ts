import { path } from "@tauri-apps/api";
import { homeDir } from "@tauri-apps/api/path";
import { platform } from "@tauri-apps/plugin-os";
import { persistedState } from "./persisted.svelte";

export const username = persistedState<string>("username", "Player");
export const onboardingCompleted = persistedState<boolean>("onboardingCompleted", false);
export const servicesURL = persistedState<string>("servicesURL", "https://api.lowrezstudio.com");
export type Theme = "system" | "mocha" | "latte" | "legacy" | "custom";
export const theme = persistedState<Theme>("theme", "system");
export const customThemeCSS = persistedState<string>("customThemeCSS", "");
export const pinnedBackground = persistedState<string | undefined>("pinnedBackground", undefined);
export const winePath = persistedState<string | undefined>("winePath", undefined);
export type WineRuntimeSetting = "auto" | "wine" | "proton";
export const wineRuntime = persistedState<WineRuntimeSetting>("wineRuntime", "auto");
export const protonPath = persistedState<string | undefined>("protonPath", undefined);
export const useSteamRuntime = persistedState<boolean>("useSteamRuntime", true);
export const useGamescope = persistedState<boolean>("useGamescope", false);
export const gamescopeArgs = persistedState<string>("gamescopeArgs", "-f --force-grab-cursor");
export const modDeveloper = persistedState<boolean>("modDeveloper", false);
export const dismissExperimentalWarning = persistedState<boolean>(
	"dismissExperimentalWarning",
	false,
);

const getDefaultInstancePath = async () => {
	const rootDir = platform() === "windows" ? "C:" : await homeDir();

	return await path.join(rootDir, "Games", "Tempest");
};

export const defaultInstancePath = persistedState<string | undefined>(
	"defaultInstancePath",
	undefined,
);

if (typeof window !== "undefined") {
	if (!defaultInstancePath.get()) {
		void getDefaultInstancePath().then((p) => {
			defaultInstancePath.set(p);
		});
	}
}
