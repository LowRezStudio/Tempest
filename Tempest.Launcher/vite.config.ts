import { paraglideVitePlugin } from "@inlang/paraglide-js";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

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
