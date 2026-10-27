import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

/** The files `@ruffle-rs/ruffle` loads at runtime; see `environments.client` below. */
const RUFFLE_FILE = /^(ruffle\.js|core\.ruffle\..+\.js|.+\.wasm|LICENSE_\w+)$/;

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	// Pre-bundle the heavy dependencies so the dev server never discovers them
	// mid-session (that triggers a full page reload, which drops the open
	// workspace).
	optimizeDeps: {
		include: [
			"three",
			"three/examples/jsm/controls/OrbitControls.js",
			"three/examples/jsm/loaders/GLTFLoader.js",
			"codemirror",
			"@codemirror/state",
			"@codemirror/view",
			"@codemirror/language",
			"@codemirror/commands",
			"@codemirror/search",
			"@codemirror/legacy-modes/mode/clike",
			"@codemirror/theme-one-dark",
		],
		// Ruffle is a prebuilt webpack bundle; esbuild must not touch it.
		exclude: ["@ruffle-rs/ruffle"],
	},
	clearScreen: false,
	environments: {
		client: {
			build: {
				rollupOptions: {
					output: {
						// Ruffle's self-hosted bundle (imported with `?url` in
						// src/lib/studio/ruffle.ts) builds its own chunk and wasm URLs from
						// fixed file names, so those files must keep them; the rest keep
						// SvelteKit's immutable, hashed names.
						assetFileNames: ({ names }) => {
							const name = names[0] ?? "";
							return RUFFLE_FILE.test(name)
								? "ruffle/[name][extname]"
								: "_app/immutable/assets/[name].[hash][extname]";
						},
					},
				},
			},
		},
	},
	server: {
		port: 1421,
		strictPort: true,
		host: false,
		watch: {
			ignored: ["**/electron/**"],
		},
	},
});
