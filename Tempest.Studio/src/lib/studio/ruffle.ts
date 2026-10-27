// The self-hosted Ruffle player: a prebuilt webpack bundle that registers
// `window.RufflePlayer` and resolves its core chunk and wasm builds relative to its
// own URL. The files are imported as URLs (served from `@ruffle-rs/ruffle` in dev,
// emitted next to the bundle in a build) and the script is loaded on first play;
// the wasm is ~28 MB.
//
// Only the parts of Ruffle's JS API the player uses are typed.

import ruffleScriptUrl from "@ruffle-rs/ruffle/ruffle.js?url";

/**
 * The files the bundle asks for at runtime. It builds their URLs itself, so the
 * imports are only here to make a build emit them beside `ruffle.js`.
 */
import.meta.glob(
	[
		"/node_modules/@ruffle-rs/ruffle/core.ruffle.*.js",
		"/node_modules/@ruffle-rs/ruffle/*.wasm",
		"/node_modules/@ruffle-rs/ruffle/LICENSE_*",
	],
	{ query: "?url", import: "default", eager: true },
);

/** The `<ruffle-player>` custom element Ruffle creates. */
export type RuffleElement = HTMLElement & {
	ruffle(): {
		/** Per-player options (`wmode`, `scale`, …) override the global config. */
		load(
			options: ({ url: string } | { data: Uint8Array; swfFileName?: string }) &
				Record<string, unknown>,
		): Promise<void>;
	};
	reload(): Promise<void>;
	play(): void;
	pause(): void;
	readonly isPlaying: boolean;
	volume: number;
	/** Ruffle enlarges its own element; the promise is not awaited. */
	requestFullscreen(): Promise<void>;
};

type RuffleSource = { newest(): { createPlayer(): RuffleElement } };

declare global {
	interface Window {
		RufflePlayer?: { config?: Record<string, unknown> } & Partial<RuffleSource>;
	}
}

/** Ruffle merges this into its defaults when the player loads. */
function config(publicPath: string): Record<string, unknown> {
	return {
		publicPath,
		autoplay: "on",
		unmuteOverlay: "hidden",
		letterbox: "on",
		scale: "showAll",
		quality: "high",
		allowScriptAccess: "never",
		allowNetworking: "none",
		openUrlMode: "deny",
		splashScreen: false,
		logLevel: "error",
	};
}

let pending: Promise<RuffleSource> | null = null;

/** Virtual movies served to Ruffle, by the pathname it requests. */
const routes = new Map<string, Uint8Array>();

/**
 * An empty movie (zero-size stage, one empty frame). A movie can import a shared library
 * the workspace does not contain — the alpha packages, for one, import
 * `gfximagestack` but never ship it. Ruffle treats a failed import as fatal and
 * paints a blank white stage, which looks like a broken preview; answering with
 * an empty movie keeps the importing movie playing with those symbols missing.
 */
const EMPTY_MOVIE = Uint8Array.from([
	0x46, 0x57, 0x53, 0x0a, 0x11, 0x00, 0x00, 0x00, 0x00, 0x18, 0x00, 0x01, 0x00, 0x40, 0x00, 0x00,
	0x00,
]);
let shimInstalled = false;

/**
 * Ruffle resolves `ImportAssets2` URLs (the Scaleform shared libraries) with
 * `fetch`, against the page URL. Studio has no files for them, so the player
 * serves the bytes the backend found in the workspace from an in-memory map;
 * everything else stays on the normal network path.
 */
function installFetchShim(): void {
	if (shimInstalled) return;
	shimInstalled = true;
	const original = window.fetch.bind(window);
	window.fetch = (input: RequestInfo | URL, init?: RequestInit) => {
		let url: URL | null = null;
		try {
			const href =
				typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
			url = new URL(href, document.baseURI);
		} catch {
			url = null;
		}
		const bytes = url ? routes.get(url.pathname) : undefined;
		if (url && bytes) {
			return Promise.resolve(
				new Response(bytes as unknown as BodyInit, {
					headers: { "content-type": "application/x-shockwave-flash" },
				}),
			);
		}
		// A same-origin movie that was not registered is an import the workspace
		// does not have. A 404 makes Ruffle drop the whole stage, so serve the
		// empty movie instead and let it report the missing symbols only.
		if (url && url.origin === location.origin && /\.(swf|gfx)$/i.test(url.pathname)) {
			return Promise.resolve(
				new Response(EMPTY_MOVIE as unknown as BodyInit, {
					headers: { "content-type": "application/x-shockwave-flash" },
				}),
			);
		}
		return original(input, init);
	};
}

/**
 * Serve a movie and its imported libraries to Ruffle. The movie gets a private
 * URL so its own request never touches the network; the libraries are served
 * at the page-relative paths Ruffle asks for (`/SharedAssets/SharedAssets.swf`).
 */
export function registerMovie(
	movie: Uint8Array,
	libraries: { url: string; swf: Uint8Array }[],
): { url: string; dispose(): void } {
	installFetchShim();
	const token =
		crypto.randomUUID?.() ??
		`${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
	const url = new URL(`/swf-preview/${token}/movie/movie.gfx`, document.baseURI);
	const served = new Map<string, Uint8Array>();
	served.set(url.pathname, movie);
	for (const library of libraries) {
		try {
			// Ruffle resolves ImportAssets2 against the document, not the movie.
			const imported = new URL(library.url, document.baseURI);
			// External URLs stay Ruffle's business (and normally fail offline).
			if (imported.origin === url.origin) served.set(imported.pathname, library.swf);
		} catch {
			// A malformed import URL is simply not served.
		}
	}
	for (const [pathname, bytes] of served) routes.set(pathname, bytes);
	return {
		url: url.pathname,
		dispose() {
			for (const pathname of served.keys()) routes.delete(pathname);
		},
	};
}

/**
 * Load Ruffle's ruffle.js once per window.
 *
 * The bundle resolves its core chunk and wasm relative to its own URL, so the
 * script URL and the config's `publicPath` both point at the directory Vite serves
 * it from.
 */
export function loadRuffle(): Promise<RuffleSource> {
	if (window.RufflePlayer?.newest) return Promise.resolve(window.RufflePlayer as RuffleSource);
	if (pending) return pending;

	const scriptUrl = new URL(ruffleScriptUrl, document.baseURI);
	const directory = new URL(".", scriptUrl).href;
	window.RufflePlayer ??= {};
	window.RufflePlayer.config = config(directory);

	pending = new Promise<RuffleSource>((resolve, reject) => {
		const script = document.createElement("script");
		script.src = scriptUrl.href;
		script.async = true;
		script.onload = () => {
			const ruffle = window.RufflePlayer;
			if (ruffle?.newest) resolve(ruffle as RuffleSource);
			else reject(new Error("ruffle.js loaded but did not register RufflePlayer"));
		};
		script.onerror = () => reject(new Error(`could not load ${script.src}`));
		document.head.append(script);
	}).catch((error: unknown) => {
		pending = null;
		// A failed script must not leave a half-initialised player behind.
		window.RufflePlayer = undefined;
		throw error;
	});

	return pending;
}
