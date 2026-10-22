import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

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
	},
	clearScreen: false,
	server: {
		port: 1421,
		strictPort: true,
		host: false,
		watch: {
			ignored: ["**/electron/**"],
		},
	},
});
