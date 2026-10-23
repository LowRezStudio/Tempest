import { updateInstance } from "../stores/instance.svelte";
import { persistedJSON } from "../stores/persisted.svelte";
import type { Instance, LaunchPreset } from "../types/instance";

// Presets are shared by every instance; only the selection is per instance.
export const launchPresets = persistedJSON<LaunchPreset[]>("launchPresets", []);

export const activePreset = (instance: Instance): LaunchPreset | undefined =>
	launchPresets.value.find((preset) => preset.id === instance.launchOptions.presetId);

// Preset arguments run first, the instance's own (Settings) arguments after them.
export const launchArgs = (instance: Instance): string[] => [
	...(activePreset(instance)?.args ?? []),
	...(instance.launchOptions.args ?? []),
];

export const saveLaunchPresets = (presets: LaunchPreset[]) => {
	launchPresets.value = presets;
};

// Selecting the active preset again clears it, leaving just the instance's own args.
export const applyLaunchPreset = (instance: Instance, preset: LaunchPreset) =>
	updateInstance(instance.id, {
		launchOptions: {
			...instance.launchOptions,
			presetId: instance.launchOptions.presetId === preset.id ? undefined : preset.id,
		},
	});
