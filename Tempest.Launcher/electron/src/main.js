const { app, BrowserWindow } = require("electron");
const path = require("node:path");
const shellMod = require("./shell");
require("./fs");
require("./dialog");
require("./opener");
require("./os");
require("./path");
require("./app");
require("./window");
require("./scopes");
require("./sql");
require("./updater");

let mainWindow = null;

process.chdir(process.cwd());

function createWindow() {
	mainWindow = new BrowserWindow({
		width: 1280,
		height: 900,
		minWidth: 1024,
		minHeight: 800,
		title: "Tempest",
		webPreferences: {
			preload: path.join(__dirname, "preload.js"),
			contextIsolation: true,
			nodeIntegration: false,
			sandbox: false,
		},
	});

	if (!app.isPackaged) {
		void mainWindow.loadURL("http://localhost:1420");
	} else {
		void mainWindow.loadFile(path.join(__dirname, "..", "build", "index.html"));
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

app.commandLine.appendSwitch("js-flags", "--experimental-sqlite");

app.whenReady()
	.then(() => {
		createWindow();
	})
	.catch(console.error);

app.on("window-all-closed", () => {
	app.quit();
});

app.on("will-quit", () => {
	for (const [, child] of shellMod.activeChildren) {
		try {
			child.kill();
		} catch {
			/* ignore */
		}
	}
	shellMod.activeChildren.clear();
});
