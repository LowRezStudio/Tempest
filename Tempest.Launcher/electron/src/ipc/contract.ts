// Single source of truth for the renderer <-> host IPC surface: every
// `invoke` channel with its argument and result types, every event channel
// the host can push to the renderer, and the shape of the preload bridge.
//
// This file is TYPES ONLY (no runtime values) on purpose:
// - the main process registers handlers against it via `ipc/handle.ts`
// - the renderer consumes it through the typed `window.electronAPI` bridge
//   (declared in src/lib/platform/globals.d.ts) with `import type`, so the
//   bundler never pulls this file into renderer code.
//
// Channel naming: `<domain>:<verb>` for invoke channels.

// ---- Shared value types ----

/** Platforms an instance's game install can target. */
export type GamePlatform = "Win64" | "Win32";

/** Where a Paladins install on this machine was found. */
export type InstallationSource = "steam" | "epic" | "hirez" | "tempest" | "folder";

export type FoundInstallation = { path: string; source: InstallationSource };

export type SpawnOptions = { env?: Record<string, string> };

export type ShellExecuteResult = {
	code: number | null;
	signal: string | null;
	stdout: string;
	stderr: string;
};

/** Demultiplexed child-process events for spawned shells (see `shell:process`). */
export type ShellProcessEvent =
	| { pid: number; type: "stdout"; data: string }
	| { pid: number; type: "stderr"; data: string }
	| { pid: number; type: "close"; code: number | null; signal: string | null }
	| { pid: number; type: "error"; message: string };

/** A tray context-menu entry. Only serializable data crosses the bridge. */
export type TrayEntry =
	| { kind: "item"; id: TrayActionId; text: string; enabled: boolean }
	| { kind: "separator" };

/** Well-known tray item ids handled entirely by the host. */
export type TrayHostActionId = "open" | "quit";
/** Nav items: the host shows the window and pushes the route to the renderer. */
export type TrayNavId = `nav:${string}`;
/** Any tray menu item id: host-handled actions or `nav:<route>`. */
export type TrayActionId = TrayHostActionId | TrayNavId;

/** Declarative tray state pushed by the renderer; the host renders it. */
export type TrayState = { entries: TrayEntry[] };

export type InstancePaths = {
	base: string;
	tokensDir: string;
	fieldsToken: string;
	functionsToken: string;
	assemblyDb: string;
};

/** Everything instance setup needs to know about the game install, resolved host-side. */
export type GameLayout = {
	gameFolder: string;
	tokenSources: Record<GamePlatform, { primary: string; fallback: string }>;
	assemblyDat: string;
};

export type DialogFilter = { name: string; extensions: string[] };
export type OpenDialogOptions = {
	directory?: boolean;
	multiple?: boolean;
	filters?: DialogFilter[];
	title?: string;
	defaultPath?: string;
};
export type SaveDialogOptions = {
	defaultPath?: string;
	filters?: DialogFilter[];
	title?: string;
};

export type UpdaterProgress = { chunkLength: number; contentLength: number };

/** What the renderer should do after the user picks a log-console menu item. */
export type ContextMenuAction = "copy" | "selectAll";

/** Options for the host's native text context menu (log console, editable fields). */
export type ContextMenuOptions = {
	/** The right-clicked element accepts text input. */
	editable: boolean;
	/** There is a text selection that Copy/Cut can act on. */
	canCopy: boolean;
	/** The surface can select its whole (virtualized) contents. */
	canSelectAll: boolean;
	/** Localized item labels: the host has no message catalog. */
	labels: { copy: string; selectAll: string };
};

export type LegacyInstall = { id: string; name: string | null };
export type LegacyUninstallResult = { id: string; name: string | null; code: number };

/** Which mod file "show in explorer" should reveal. */
export type ModLocation = { kind: "V2"; modId: string } | { kind: "V1"; targetPath: string };

export type ModPackageInfo = {
	id: string;
	name: string;
	version: string;
	authors: string[];
	/** The manifest's `icon` path inside the package, when it names one. */
	icon?: string;
	/** `[mod].dependencies` entries as written, e.g. "tempest-scripting>=1.0.0" */
	dependencies: string[];
};

/** A file the V1→V2 converter imports, read host-side in one round trip. */
export type ConverterImport = { path: string; name: string; data: ArrayBuffer };

/** One file inside a zip container handled by the host (see `zip:build`). */
export type ZipEntry = { path: string; data: Uint8Array };

/** Keybinds mod settings, cooked into an INI package by `keybinds:cook`. */
export type Keybind = { name: string; command: string };
export type KeybindsConfig = {
	mouseSensitivity: number;
	fpsLimit: number;
	bindings: Keybind[];
};

/** Result of installing the `tempest` CLI shim into a PATH directory. */
export type CliInstallResult = {
	/** Whether the shim exists at `path` after the call. */
	installed: boolean;
	path: string | null;
	/** Whether the shim's directory is on the PATH we can see. */
	onPath: boolean;
	/** Why nothing was installed: "cli-not-bundled" (dev run) or an error message. */
	error?: string;
};

/** "cancelled": the UAC prompt was declined; "failed" carries what PowerShell or Defender said. */
export type DefenderExclusionResult = {
	status: "added" | "cancelled" | "failed";
	error?: string;
};

/** OS info injected by preload/host (see `window.__os`). */
export type OsInfo = {
	platform: "windows" | "macos" | "linux";
	arch: string;
	type: string;
	version: string;
};

/**
 * The typed invoke surface. `args: void` channels take no payload.
 * Handlers may reject; the renderer sees the rejection as an `Error`.
 */
export interface IpcContract {
	// ---- App / settings ----
	"app:version": { args: void; result: string };
	"app:relaunch": { args: void; result: void };
	"app:take-pending-open-files": { args: void; result: string[] };
	"app:take-pending-deep-links": { args: void; result: string[] };
	"app:default-instance-path": { args: void; result: string };
	"app:set-close-behavior": { args: { hideToTray: boolean }; result: void };
	"settings:write-snapshot": { args: { json: string }; result: void };
	"settings:delete-snapshot": { args: void; result: void };
	/** Renderer state the host keeps in memory so it survives a page reload. */
	"state:set": { args: { patch: Record<string, unknown> }; result: void };

	// ---- Window ----
	"window:destroy": { args: void; result: void };
	"window:hide": { args: void; result: void };
	"window:show": { args: void; result: void };
	"window:set-focus": { args: void; result: void };
	"window:unminimize": { args: void; result: void };
	"window:context-menu": { args: ContextMenuOptions; result: ContextMenuAction | null };

	// ---- Tray (host owns the TrayIcon + Menu; renderer pushes state) ----
	"tray:ensure": { args: { tooltip: string }; result: void };
	"tray:set-state": { args: { entries: TrayEntry[] }; result: void };

	// ---- Shell ----
	"shell:spawn": {
		args: { program: string; args: string[]; options?: SpawnOptions; sidecar?: boolean };
		result: { pid: number };
	};
	/** With `streamId`, stderr is also forwarded as it arrives, as `shell:execute-stderr` events tagged with it. */
	"shell:execute": {
		args: {
			program: string;
			args: string[];
			options?: SpawnOptions;
			sidecar?: boolean;
			streamId?: string;
		};
		result: ShellExecuteResult;
	};
	"shell:kill": { args: { pid: number }; result: void };
	"shell:stdin-write": { args: { pid: number; data: string }; result: void };
	"shell:which": { args: { name: string }; result: string | null };
	/** Children still alive in the host; a renderer reload loses its own process list. */
	"shell:list": { args: void; result: number[] };

	// ---- Path helpers (cross-platform correctness over IPC) ----
	"path:join": { args: { paths: string[] }; result: string };
	"path:dirname": { args: { path: string }; result: string };
	"path:resolve-resource": { args: { paths: string[] }; result: string };

	// ---- Dialogs ----
	"dialog:open": { args: OpenDialogOptions; result: string[] | null };
	"dialog:save": { args: SaveDialogOptions; result: string | null };

	// ---- Opener ----
	"opener:open-path": { args: { path: string }; result: void };
	"opener:open-url": { args: { url: string }; result: void };

	// ---- Updater ----
	"updater:check": {
		args: void;
		result: {
			available?: boolean;
			version?: string;
			/** Markdown body of the pending GitHub release (tag annotation). */
			releaseNotes?: string | null;
			error?: string;
		};
	};
	"updater:download": { args: void; result: void };

	// ---- Legacy Tauri uninstall (Windows) ----
	"legacy:find": { args: void; result: LegacyInstall[] };
	"legacy:uninstall": { args: void; result: LegacyUninstallResult[] };

	// ---- Platforms ----
	"platforms:detect": { args: { instancePath: string }; result: GamePlatform[] };
	/**
	 * Paladins installs on this machine. `folders` are searched one level deep as Tempest's own
	 * downloads; `sweep: false` skips looking through the drives beyond the known launchers.
	 * With `progressId`, each install is also sent as a `platforms:found-paladins` event the
	 * moment it is found.
	 */
	"platforms:find-paladins": {
		args: { folders: string[]; sweep?: boolean; progressId?: string };
		result: FoundInstallation[];
	};

	// ---- Instances ----
	"instance:paths": { args: { id: string }; result: InstancePaths };
	"instance:delete-data": { args: { instancePath: string }; result: void };
	"instance:game-layout": { args: { instancePath: string }; result: GameLayout };

	// ---- Mods ----
	/**
	 * With `progressId`, reports progress as `mods:download-progress` events tagged with it.
	 * With `cache`, the package lands in the per-mod version cache and a cached copy (matching
	 * `sha256` when given) is returned without downloading.
	 */
	"mods:download-to-temp": {
		args: {
			url: string;
			filename: string;
			progressId?: string;
			cache?: { modId: string; version: string; sha256?: string };
		};
		result: string;
	};
	/** Versions of a repository mod kept in the download cache. */
	"mods:cached-versions": { args: { modId: string }; result: string[] };
	/** POST to the mod repository from the main process (no Origin header, see repo.ts). */
	"repo:post-json": {
		args: { url: string; body: unknown };
		result: { status: number; body: string };
	};
	/** The `[mod]` table of a V2 package's manifest.toml; null for V1 files or unreadable ones. */
	"mods:read-manifest": { args: { path: string }; result: ModPackageInfo | null };
	"mods:stage-pack": { args: { filename: string; data: Uint8Array }; result: string };
	"mods:save-staged-pack": { args: { stagedPath: string }; result: string | null };
	"mods:save-staged-batch": {
		args: { files: { stagedPath: string; filename: string }[] };
		result: { dir: string; saved: string[] } | null;
	};
	"mods:discard-staged-pack": { args: { stagedPath: string }; result: void };
	/**
	 * Copies a package into the install queue's folder (keeping its file name), so a queued
	 * install still has it when it runs, whatever happens to the original. Returns the copy.
	 */
	"mods:queue-stage": { args: { path: string }; result: string };
	/** Deletes a copy `mods:queue-stage` made; other paths are ignored. */
	"mods:queue-discard": { args: { path: string }; result: void };
	/** Deletes every copy in the install queue's folder except `keep`. */
	"mods:queue-prune": { args: { keep: string[] }; result: void };
	"mods:open-location": { args: { gamePath: string; mod: ModLocation }; result: void };
	"mods:has-dlls": { args: { gamePath: string; modId: string }; result: boolean };
	/** The per-mod TOML config, or null when the mod has no config file. */
	"mods:read-config": { args: { gamePath: string; modId: string }; result: string | null };
	/** Replaces an installed mod's existing TOML config. */
	"mods:write-config": { args: { gamePath: string; modId: string; text: string }; result: void };
	/** An installed V2 mod's own icon (manifest `icon`, else icon.png) as a data URL; null without one. */
	"mods:installed-icon": { args: { gamePath: string; modId: string }; result: string | null };
	"mods:read-file": {
		args: { basePath: string; modId: string | undefined; path: string };
		result: { text: string; truncated: boolean };
	};

	// ---- Converter (V1 -> V2) ----
	"converter:pick-imports": { args: void; result: ConverterImport[] };
	"converter:read-imports": { args: { paths: string[] }; result: ConverterImport[] };
	"converter:pick-icon": { args: void; result: ConverterImport | null };

	// ---- Zip container (mod cooker: fflate reads and writes the packages) ----
	"zip:build": { args: { entries: { path: string; data: Uint8Array }[] }; result: Uint8Array };
	"zip:read": { args: { data: Uint8Array }; result: ZipEntry[] };

	// ---- upk-parser (bundled zig tool; UPK compression for the mod cooker) ----
	"upk:optimize": {
		args: { path: string };
		result: { optimized: boolean; data: ArrayBuffer | null };
	};
	"upk:optimize-data": {
		args: { name: string; data: Uint8Array };
		result: { optimized: boolean; data: ArrayBuffer | null };
	};

	// ---- Keybinds mod (INI + manifest + zip cooked host-side) ----
	/** Stages the cooked package in the temp dir and returns its path. */
	"keybinds:cook": {
		args: { name: string; version: string; config: KeybindsConfig };
		result: string;
	};

	// ---- Mod publishing setup (signing key + repository API key) ----
	"signing:status": { args: void; result: { hasKey: boolean; keyId: string | null } };
	"repo:status": { args: void; result: { hasApiKey: boolean; masked: string | null } };
	"repo:forget": { args: void; result: void };

	// ---- CLI shim (`tempest <command>` in a terminal) ----
	"cli:install": { args: void; result: CliInstallResult };
	"cli:status": { args: void; result: CliInstallResult };

	// ---- Windows Defender (folders the CLI reported as blocked; prompts for UAC) ----
	"defender:add-exclusions": { args: { paths: string[] }; result: DefenderExclusionResult };
}

/** Channels the host pushes to the renderer. */
export interface IpcEvents {
	/** `.tempest` files queued by open-file/second-instance; nudge the renderer. */
	"open-mod-files": null;
	/** `tempest://` URLs queued by open-url/second-instance; nudge the renderer. */
	"deep-link": null;
	/** Navigate the SPA to a route (tray menu, host-handled). */
	"tray:navigate": { route: string };
	/** Child-process stream events, demultiplexed by pid. */
	"shell:process": ShellProcessEvent;
	"shell:execute-stderr": { id: string; data: string };
	"updater:download-progress": UpdaterProgress;
	/** `total` is null when the server sends no Content-Length. */
	"mods:download-progress": { id: string; received: number; total: number | null };
	"platforms:found-paladins": { id: string; install: FoundInstallation };
}

export type IpcChannel = Extract<keyof IpcContract, string>;
export type IpcEventChannel = Extract<keyof IpcEvents, string>;

/** The object the preload script exposes as `window.electronAPI`. */
export interface PreloadApi {
	invoke<K extends IpcChannel>(
		channel: K,
		...args: IpcContract[K]["args"] extends void ? [] : [IpcContract[K]["args"]]
	): Promise<IpcContract[K]["result"]>;
	on<K extends IpcEventChannel>(
		channel: K,
		callback: (payload: IpcEvents[K]) => void,
	): () => void;
	getPathForFile(file: File): string;
}
