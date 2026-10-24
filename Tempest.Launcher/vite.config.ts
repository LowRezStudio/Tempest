import { fileURLToPath } from "node:url";
import { paraglideVitePlugin } from "@inlang/paraglide-js";
import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

const resolvePath = (path: string) => fileURLToPath(new URL(path, import.meta.url));

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
		sveltekit({
			// adapter-auto only supports some environments, see https://svelte.dev/docs/kit/adapter-auto for a list.
			// If your environment is not supported, or you settled on a specific environment, switch out the adapter.
			// See https://svelte.dev/docs/kit/adapters for more information about adapters.
			adapter: adapter({
				fallback: "index.html",
			}),
			paths: {
				relative: true,
			},
		}),
	],

	// SvelteKit 3 dropped the implicit $lib alias; declare it here so existing imports keep working
	resolve: {
		alias: {
			$lib: resolvePath("./src/lib"),
			// The typed IPC contract shared with the host lives outside src/lib
			$ipc: resolvePath("./electron/src/ipc"),
		},
	},

	define: {
		__BUILD_DATE__: JSON.stringify(new Date().toISOString()),
	},

	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		host: false,
		watch: {
			ignored: ["**/electron/**"],
		},
	},
});
