import path from "node:path";
import { paraglideVitePlugin } from "@inlang/paraglide-js";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

const electronAliases: Record<string, string> = process.env.ELECTRON
	? {
			// Most specific paths first — Vite prefix-matches and appends the remainder
			"@tauri-apps/api/window": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/api/path": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/api/core": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/api/app": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/plugin-updater": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/plugin-shell": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/plugin-opener": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/plugin-os": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/plugin-http": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/plugin-fs": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/plugin-sql": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/plugin-dialog": path.resolve("src/lib/electron/bridge"),
			"@tauri-apps/api": path.resolve("src/lib/electron/bridge"),
		}
	: {};

// https://vitejs.dev/config/
export default defineConfig({
	plugins: [
		paraglideVitePlugin({
			project: "./project.inlang",
			outdir: "./src/lib/paraglide",
			strategy: ["localStorage", "baseLocale"],
			disableAsyncLocalStorage: true,
		}),
		tailwindcss(),
		sveltekit(),
	],

	define: {
		__BUILD_DATE__: JSON.stringify(new Date().toISOString()),
	},

	resolve: {
		alias: electronAliases,
	},

	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		host: host ?? false,
		hmr: host
			? {
					protocol: "ws",
					host,
					port: 1421,
				}
			: undefined,
		watch: {
			ignored: ["**/src-tauri/**"],
		},
	},
});
