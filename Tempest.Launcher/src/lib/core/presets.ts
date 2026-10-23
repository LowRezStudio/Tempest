import { updateInstance } from "$lib/stores/instance.svelte";
import type { Instance, LaunchPreset } from "$lib/types/instance";

// Presets saved on the instance (instances created before presets existed have none).
export const launchPresets = (instance: Instance): LaunchPreset[] =>
	instance.launchOptions.presets ?? [];

export const activePreset = (instance: Instance): LaunchPreset | undefined =>
	launchPresets(instance).find((preset) => preset.id === instance.launchOptions.presetId);

// Preset arguments run first, the instance's own (Settings) arguments after them.
export const launchArgs = (instance: Instance): string[] => [
	...(activePreset(instance)?.args ?? []),
	...(instance.launchOptions.args ?? []),
];

export const saveLaunchPresets = (instance: Instance, presets: LaunchPreset[]) =>
	updateInstance(instance.id, {
		launchOptions: { ...instance.launchOptions, presets },
	});

// Selecting the active preset again clears it, leaving just the instance's own args.
export const applyLaunchPreset = (instance: Instance, preset: LaunchPreset) =>
	updateInstance(instance.id, {
		launchOptions: {
			...instance.launchOptions,
			presetId: instance.launchOptions.presetId === preset.id ? undefined : preset.id,
		},
	});
