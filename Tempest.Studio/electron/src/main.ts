// Electron main entry of Tempest Studio: windows (one `upk serve` backend
// each, so every window can work on its own game folder), file associations
// for .upk files. IPC handlers live in their own modules and register
// themselves when imported.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { app, BrowserWindow, session } from "electron";
import serve from "electron-serve";
import "./dialog.js";
import "./system.js";
import { handle } from "./ipc/handle.js";
import { onCreateWindow, startMcp } from "./mcp.js";
import { installSmoke } from "./smoke.js";
import { upkFor } from "./upk.js";
import type { WindowStartup } from "./ipc/contract.js";

const DEV_URL = "http://localhost:1421";

/** Files opened from the OS while no window could take them yet. */
let pendingFiles: string[] = [];
let smokeInstalled = false;
/** What each window (by WebContents id) opens once its renderer asks; the
 * renderer pulls it, so nothing is lost to a reload or a late listener. */
const startups = new Map<number, WindowStartup>();
/** Windows whose renderer has asked for its startup: events reach them. */
const ready = new Set<number>();

function upkFilesFromArgs(args: string[], workingDirectory: string): string[] {
	return args.flatMap((argument) => {
		let filePath: string;
		try {
			filePath = argument.startsWith("file:")
				? fileURLToPath(argument)
				: path.isAbsolute(argument)
					? argument
					: path.resolve(workingDirectory, argument);
		} catch {
			return [];
		}
		const ext = path.extname(filePath).toLowerCase();
		return (ext === ".upk" || ext === ".u") && fs.existsSync(filePath) ? [filePath] : [];
	});
}

/** The window an OS request goes to: the focused one, else the last one created. */
function targetWindow(): BrowserWindow | null {
	const focused = BrowserWindow.getFocusedWindow();
	if (focused && !focused.isDestroyed()) return focused;
	const all = BrowserWindow.getAllWindows().filter((w) => !w.isDestroyed());
	return all.at(-1) ?? null;
}

function queueOpenFiles(files: string[]): void {
	if (files.length === 0) return;
	const window = targetWindow();
	if (!window) {
		pendingFiles.push(...files);
		return;
	}
	const id = window.webContents.id;
	if (ready.has(id) && !window.webContents.isLoading()) {
		window.webContents.send("open-files", files);
	} else {
		const startup = startups.get(id) ?? {};
		startups.set(id, { ...startup, files: [...(startup.files ?? []), ...files] });
	}
	if (window.isMinimized()) window.restore();
	window.show();
	window.focus();
}

queueOpenFiles(upkFilesFromArgs(process.argv.slice(1), process.cwd()));

app.on("open-file", (event, filePath) => {
	event.preventDefault();
	queueOpenFiles(upkFilesFromArgs([filePath], process.cwd()));
});

const hasInstanceLock = app.requestSingleInstanceLock();
if (!hasInstanceLock) {
	app.quit();
} else {
	app.on("second-instance", (_event, commandLine, workingDirectory) => {
		const files = upkFilesFromArgs(commandLine, workingDirectory);
		if (files.length > 0) {
			queueOpenFiles(files);
			return;
		}
		const window = targetWindow();
		if (window) {
			if (window.isMinimized()) window.restore();
			window.show();
			window.focus();
		} else {
			createWindow();
		}
	});
}

const loadURL = serve({ directory: "build" });

function createWindow(options: WindowStartup = {}): BrowserWindow {
	const icon = app.isPackaged
		? path.join(process.resourcesPath, "icon.png")
		: path.join(import.meta.dirname, "..", "resources", "icons", "icon.png");

	// Cascade from the window the request came from.
	const anchor = targetWindow();
	const position = anchor ? anchor.getPosition() : null;

	// Dev harness: STUDIO_SMOKE_SIZE=WxH opens the window at that size.
	const size = /^(\d+)x(\d+)$/.exec(process.env.STUDIO_SMOKE_SIZE ?? "");
	const window = new BrowserWindow({
		width: size ? Number(size[1]) : 1600,
		height: size ? Number(size[2]) : 950,
		// Small laptops at 125 % scaling have about 1090 × 650 DIP to give.
		minWidth: 900,
		minHeight: 560,
		...(position ? { x: position[0] + 40, y: position[1] + 40 } : {}),
		title: "Tempest Studio",
		icon,
		autoHideMenuBar: true,
		backgroundColor: "#1e1e2e",
		webPreferences: {
			preload: path.join(import.meta.dirname, "preload.cjs"),
			contextIsolation: true,
			nodeIntegration: false,
			sandbox: true,
			devTools: true,
		},
	});
	// The screenshot harness drives the first window only.
	if (!app.isPackaged && !smokeInstalled) {
		smokeInstalled = true;
		installSmoke(window);
	}
	upkFor(window.webContents).start();

	const files = [...pendingFiles, ...(options.files ?? [])];
	pendingFiles = [];
	if (options.dir || files.length > 0) {
		startups.set(window.webContents.id, { ...options, files });
	}

	window.webContents.on("will-navigate", (event, url) => {
		try {
			const u = new URL(url);
			const sameOrigin = app.isPackaged
				? u.protocol === "app:"
				: `${u.protocol}//${u.host}` === DEV_URL;
			if (!sameOrigin) event.preventDefault();
		} catch {
			event.preventDefault();
		}
	});
	window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));

	if (!app.isPackaged) {
		void window.loadURL(DEV_URL);
	} else {
		void loadURL(window);
	}
	return window;
}

handle("window:new", (_event, options) => {
	createWindow(options ?? {});
});

handle("window:startup", (event, { done }) => {
	const id = event.sender.id;
	ready.add(id);
	const startup = startups.get(id) ?? null;
	if (done) startups.delete(id);
	return startup;
});

app.whenReady()
	.then(() => {
		if (!hasInstanceLock) return;
		// Deny every permission request (camera, notifications, ...): the app needs none.
		session.defaultSession.setPermissionRequestHandler((_wc, _permission, callback) =>
			callback(false),
		);
		createWindow();
		onCreateWindow(createWindow);
		startMcp();
		app.on("activate", () => {
			if (BrowserWindow.getAllWindows().length === 0) createWindow();
		});
	})
	.catch((error: unknown) => {
		console.error(error);
		app.quit();
	});

app.on("window-all-closed", () => {
	app.quit();
});
