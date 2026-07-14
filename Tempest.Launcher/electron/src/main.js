import path from "node:path";
import { app, BrowserWindow, shell } from "electron";
import { activeChildren } from "./shell.js";
import "./fs.js";
import "./dialog.js";
import "./opener.js";
import "./os.js";
import "./path.js";
import "./app.js";
import "./window.js";
import "./scopes.js";
import "./sql.js";
import "./updater.js";

let mainWindow = null;

process.chdir(process.cwd());

function createWindow() {
	mainWindow = new BrowserWindow({
		width: 1280,
		height: 900,
		minWidth: 1024,
		minHeight: 800,
		title: "Tempest",
		autoHideMenuBar: true,
		webPreferences: {
			preload: path.join(import.meta.dirname, "preload.cjs"),
			contextIsolation: true,
			nodeIntegration: false,
			sandbox: true,
			devTools: !app.isPackaged,
		},
	});

	mainWindow.webContents.on("will-navigate", (event, url) => {
		const allowed = app.isPackaged ? ["file://"] : ["http://localhost:1420"];
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
		void mainWindow.loadFile(path.join(import.meta.dirname, "..", "build", "index.html"));
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
