// Keybinds mod cooker: the renderer sends only the settings, the host writes the
// INI files, the manifest and the package, and stages it in the temp dir.
import { handle } from "./ipc/handle.js";
import { stagePack } from "./mods.js";
import { buildZip } from "./zip.js";
import type { KeybindsConfig } from "./ipc/contract.js";

function buildDefaultInputIni(config: KeybindsConfig): string {
	const lines = [
		"[Engine.PlayerInput]",
		`MouseSensitivity=${Number(config.mouseSensitivity).toFixed(1)}`,
	];
	lines.push("", "[TgGame.TgPlayerInput]");
	for (const binding of config.bindings) {
		const name = binding.name.trim();
		const command = binding.command.trim();
		if (!name || !command) continue;
		lines.push(`+Bindings=(Name="${name}",    Command="${command}")`);
	}
	return `${lines.join("\n")}\n`;
}

function buildDefaultEngineIni(config: KeybindsConfig): string {
	return `[Engine.GameEngine]\nMaxSmoothedFrameRate=${config.fpsLimit}\n`;
}

function buildManifest(name: string, version: string): string {
	return `[mod]\nid = "${name}"\nname = "${name}"\nversion = "${version}"\n\n[[mod.authors]]\nname = "Tempest"\n`;
}

export function cookKeybindsMod(
	name: string,
	version: string,
	config: KeybindsConfig,
): Promise<string> {
	const modName = name.trim() || "keybinds";
	const entries = [
		{ path: "files/ChaosGame/Config/DefaultInput.ini", data: buildDefaultInputIni(config) },
		{ path: "files/ChaosGame/Config/DefaultEngine.ini", data: buildDefaultEngineIni(config) },
		{ path: "manifest.toml", data: buildManifest(modName, version.trim() || "1.0.0") },
	];
	return stagePack(`${modName}.tempest`, buildZip(entries));
}

handle("keybinds:cook", (_event, { name, version, config }) =>
	cookKeybindsMod(name, version, config),
);
