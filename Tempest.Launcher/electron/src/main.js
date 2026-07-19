import os from "node:os";
import path from "node:path";
import { app, BrowserWindow, shell, session } from "electron";
import serve from "electron-serve";
import { injectOs } from "./os.js";
import "./fs.js";
import "./dialog.js";
import "./opener.js";
import { activeChildren } from "./shell.js";
import "./path.js";
import "./app.js";
import "./window.js";
import "./scopes.js";
import "./sql.js";
import "./updater.js";

let mainWindow = null;

process.chdir(process.cwd());

// Pin to the same dir Tauri uses so SQL/localStorage survive across runs.
if (process.platform === "linux") {
	app.setPath("userData", path.join(os.homedir(), ".local", "share", "com.lowrezstudio.tempest"));
}

const loadURL = serve({ directory: "build" });

function createWindow() {
	const icon = app.isPackaged
		? path.join(process.resourcesPath, "icon.png")
		: path.join(import.meta.dirname, "..", "..", "src-tauri", "icons", "icon.png");

	mainWindow = new BrowserWindow({
		width: 1500,
		height: 900,
		minWidth: 1024,
		minHeight: 800,
		title: "Tempest",
		icon,
		autoHideMenuBar: true,
		webPreferences: {
			preload: path.join(import.meta.dirname, "preload.cjs"),
			contextIsolation: true,
			nodeIntegration: false,
			sandbox: true,
			devTools: true,
		},
	});

	mainWindow.webContents.on("did-finish-load", () => {
		injectOs(mainWindow);
	});

	mainWindow.webContents.on("will-navigate", (event, url) => {
		const allowed = app.isPackaged ? ["app://"] : ["http://localhost:1420"];
		if (!allowed.some((p) => url.startsWith(p))) {
			event.preventDefault();
			void shell.openExternal(url);
		}
	});

	mainWindow.webContents.setWindowOpenHandler(({ url }) => {
		void shell.openExternal(url);
		return { action: "deny" };
	});

	if (!app.isPackaged) {
		void mainWindow.loadURL("http://localhost:1420");
	} else {
		void loadURL(mainWindow);
	}

	mainWindow.on("close", async (e) => {
		e.preventDefault();
		try {
			const shouldClose = await mainWindow.webContents.executeJavaScript(
				"window.__closeHook?.() ?? true",
			);
			if (shouldClose) mainWindow.destroy();
		} catch {
			mainWindow.destroy();
		}
	});
}

app.whenReady()
	.then(() => {
		const upsert = (obj, key, value) => {
			const lower = key.toLowerCase();
			for (const k of Object.keys(obj)) {
				if (k.toLowerCase() === lower) {
					obj[k] = value;
					return;
				}
			}
			obj[key] = value;
		};

		session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
			const { responseHeaders: h } = details;

			if (details.method === "OPTIONS") {
				upsert(h, "Access-Control-Allow-Origin", ["*"]);
				upsert(h, "Access-Control-Allow-Methods", ["POST, GET, OPTIONS"]);
				upsert(h, "Access-Control-Allow-Headers", ["*"]);
				callback({ responseHeaders: h, statusLine: "HTTP/1.1 204 No Content" });
				return;
			}

			upsert(h, "Content-Security-Policy", [
				"default-src 'self'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self' 'unsafe-inline'; img-src * data: blob:; media-src *; font-src * data:; connect-src *; frame-src *;",
			]);
			upsert(h, "Access-Control-Allow-Origin", ["*"]);
			callback({ responseHeaders: h });
		});
		createWindow();
	})
	.catch(console.error);

app.on("window-all-closed", () => {
	app.quit();
});

app.on("will-quit", () => {
	for (const [, child] of activeChildren) {
		try {
			child.kill();
		} catch {
			/* ignore */
		}
	}
	activeChildren.clear();
});
