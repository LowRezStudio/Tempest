// Dev-only screenshot harness: with STUDIO_SMOKE_SCRIPT set to a JS file,
// the host runs it in the renderer once the page is loaded (the renderer
// exposes `window.studio` in dev builds), waits for the returned promise,
// writes a PNG of the window to STUDIO_SMOKE_OUT and quits. Used to look at
// the rendered UI from the command line.
import fs from "node:fs";
import { app, type BrowserWindow } from "electron";

export function installSmoke(window: BrowserWindow): void {
	const script = process.env.STUDIO_SMOKE_SCRIPT;
	const out = process.env.STUDIO_SMOKE_OUT;
	if (!script || !out) return;
	// Vite's dependency optimizer reloads the page once on a cold start, so
	// wait until no load has happened for a while before driving the page.
	let lastLoad = Date.now();
	let started = false;
	window.webContents.on("did-finish-load", () => {
		lastLoad = Date.now();
		if (started) return;
		started = true;
		void (async () => {
			let status = "ok";
			try {
				const code = fs.readFileSync(script, "utf8");
				for (;;) {
					await new Promise((r) => setTimeout(r, 500));
					if (Date.now() - lastLoad < 2500) continue;
					const ready =
						(await window.webContents.executeJavaScript(
							"typeof window.studio",
							true,
						)) === "object";
					if (ready) break;
				}
				const result = await window.webContents.executeJavaScript(
					`(async () => { ${code} })()`,
					true,
				);
				console.log(`[smoke] script result: ${String(result)}`);
			} catch (error) {
				status = `script failed: ${error instanceof Error ? error.message : String(error)}`;
			}
			// Capturing can fail transiently while a WebGL frame is in flight: retry.
			for (let attempt = 0; attempt < 8; attempt++) {
				await new Promise((r) => setTimeout(r, 500));
				try {
					const image = await window.webContents.capturePage();
					fs.writeFileSync(out, image.toPNG());
					break;
				} catch (error) {
					status = `capture failed: ${error instanceof Error ? error.message : String(error)}`;
				}
			}
			console.log(`[smoke] ${status}; wrote ${out}`);
			app.exit(
				status.startsWith("capture failed") || status.startsWith("script failed") ? 1 : 0,
			);
		})();
	});
}
