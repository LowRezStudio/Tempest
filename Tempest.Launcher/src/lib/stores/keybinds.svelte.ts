import JSZip from "jszip";
import { persistedJSON } from "./persisted.svelte";

export interface Keybind {
	name: string;
	command: string;
}

export interface KeybindsConfig {
	mouseSensitivity: number;
	bindings: Keybind[];
}

export const DEFAULT_KEYBINDS: KeybindsConfig = {
	mouseSensitivity: 25,
	bindings: [
		{ name: "W", command: "GBA_MoveForward" },
		{ name: "A", command: "GBA_StrafeLeft" },
		{ name: "S", command: "GBA_MoveBackward" },
		{ name: "D", command: "GBA_StrafeRight" },
		{ name: "E", command: "GBA_Ability_1" },
		{ name: "Q", command: "GBA_Ability_2" },
		{ name: "F", command: "GBA_Ability_3" },
		{ name: "R", command: "GBA_Reload" },
		{ name: "U", command: "GBA_UpgradeMenu" },
		{ name: "I", command: "GBA_CommonCards" },
		{ name: "K", command: "GBA_SkillInfo" },
		{ name: "B", command: "GBA_Emote" },
		{ name: "T", command: "GBA_Spray" },
		{ name: "G", command: "GBA_CosmeticWheelToggle" },
		{ name: "C", command: "GBA_Toggle3p" },
	],
};

export const keybinds = persistedJSON<KeybindsConfig>("keybinds.v5", DEFAULT_KEYBINDS);

export function resetKeybinds() {
	keybinds.value = structuredClone(DEFAULT_KEYBINDS);
}

export function buildDefaultInputIni(config: KeybindsConfig): string {
	const lines: string[] = [];
	lines.push("[Engine.PlayerInput]");
	lines.push(`MouseSensitivity=${Number(config.mouseSensitivity).toFixed(1)}`);
	lines.push("");
	lines.push("[TgGame.TgPlayerInput]");
	for (const binding of config.bindings) {
		const name = binding.name.trim();
		const command = binding.command.trim();
		if (!name || !command) continue;
		lines.push(`+Bindings=(Name="${name}",    Command="${command}")`);
	}
	return lines.join("\n") + "\n";
}

export interface KeybindModOptions {
	name: string;
	version?: string;
	authors?: { name: string; link: string }[];
	readme?: string;
}

export async function cookKeybindsMod(config: KeybindsConfig, options: KeybindModOptions) {
	const zip = new JSZip();
	zip.file("files/ChaosGame/Config/DefaultInput.ini", buildDefaultInputIni(config));

	const lines: string[] = [];
	lines.push("[mod]");
	lines.push(`id = "${options.name.trim()}"`);
	lines.push(`name = "${options.name.trim()}"`);
	lines.push(`version = "${options.version?.trim() || "1.0.0"}"`);
	lines.push("");
	for (const author of options.authors ?? []) {
		if (!author.name.trim()) continue;
		lines.push("[[mod.authors]]");
		lines.push(`name = "${author.name.trim()}"`);
		if (author.link.trim()) lines.push(`link = "${author.link.trim()}"`);
		lines.push("");
	}
	zip.file("manifest.toml", lines.join("\n"));

	if (options.readme?.trim()) {
		zip.file("README.md", options.readme.trim());
	}

	return zip.generateAsync({ type: "uint8array", compression: "DEFLATE" });
}
