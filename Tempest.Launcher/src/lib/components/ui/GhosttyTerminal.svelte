<script lang="ts">
	import { Terminal, FitAddon, Ghostty } from "ghostty-web";
	import { onDestroy, onMount } from "svelte";
	import type { Child, Command } from "@tauri-apps/plugin-shell";
	import type { ProcessLog } from "$lib/types/process";

	// ── Shared WASM (loaded once, reused across terminal instances) ──
	let ghosttyReady: Promise<Ghostty> | undefined;

	function loadGhostty(): Promise<Ghostty> {
		ghosttyReady ??= (async () => {
			const wasmUrl = (await import("ghostty-web/ghostty-vt.wasm?url")).default;
			return Ghostty.load(wasmUrl);
		})();
		return ghosttyReady;
	}

	// ── Props ──
	interface Props {
		logs: ReadonlyArray<ProcessLog>;
		child?: Child | null;
		/** Real-time stdout/stderr pipe (bypasses log formatting) */
		command?: Command<string> | null;
		showPrefix?: boolean;
	}

	let { logs, child, command, showPrefix = true }: Props = $props();

	let container = $state<HTMLDivElement>();
	let term = $state<Terminal>();
	let fitAddon: FitAddon | undefined;
	let lastWrittenId = -1;

	// ── Incremental log rendering ──
	$effect(() => {
		const t = term;
		if (!t) return;

		const current = logs;

		// Detect full reset: array replaced (e.g. source filter change)
		if (current.length > 0 && lastWrittenId > 0) {
			if (current.at(0)!.id > lastWrittenId + 1 || lastWrittenId >= current.at(-1)!.id) {
				t.clear();
				lastWrittenId = -1;
			}
		}

		let chunk = "";
		let newLastId = lastWrittenId;
		for (const entry of current) {
			if (entry.id > lastWrittenId) {
				const prefix = showPrefix && entry.source ? `\x1b[90m[${entry.source}]\x1b[0m ` : "";
				const color = entry.error ? "\x1b[31m" : "";
				chunk += `${prefix}${color}${entry.line}\x1b[0m\r\n`;
				newLastId = entry.id;
			}
		}

		if (chunk) {
			const atBottom = t.getViewportY() < 1;
			t.write(chunk);
			lastWrittenId = newLastId;
			if (atBottom) t.scrollToBottom();
		}
	});

	// ── Real-time stdout/stderr pipe ──
	$effect(() => {
		const cmd = command;
		const t = term;
		if (!cmd || !t) return;

		const write = (data: string) => t.write(data);
		cmd.stdout.on("data", write);
		cmd.stderr.on("data", write);

		return () => {
			cmd.stdout.removeListener("data", write);
			cmd.stderr.removeListener("data", write);
		};
	});

	// ── Stdin → child process wiring (reactive: child may appear after mount) ──
	// `child` is a derived prop (e.g. `process?.child`) that resolves asynchronously
	// after the terminal mounts, so this can't live in onMount. Also keep
	// `term.options.disableStdin` in sync, since the input handler re-reads it on
	// every keypress (ghostty-web.js: `!this.options.disableStdin` gates onData).
	$effect(() => {
		const t = term;
		if (!t) return;
		const c = child ?? null;
		t.options.disableStdin = !c;
		if (!c) return;

		const disposable = t.onData((data) => {
			void c.write(data);
		});
		return () => disposable.dispose();
	});

	// ── Lifecycle ──
	onMount(async () => {
		const el = container;
		if (!el) return;

		const ghostty = await loadGhostty();

		// Resolve theme from CSS custom properties
		const style = getComputedStyle(el);
		const bg = style.getPropertyValue("--color-base-300").trim() || "#1e1e2e";
		const fg = style.getPropertyValue("--color-base-content").trim() || "#cdd6f4";

		term = new Terminal({
			ghostty,
			fontSize: 13,
			fontFamily: "'Ubuntu Sans Mono Variable', monospace",
			theme: { background: bg, foreground: fg },
			scrollback: 10000,
			disableStdin: !child,
		});

		term.open(el);

		fitAddon = new FitAddon();
		term.loadAddon(fitAddon);
		fitAddon.fit();
		fitAddon.observeResize();
	});

	onDestroy(() => {
		fitAddon?.dispose();
		term?.dispose();
	});
</script>

<div bind:this={container} class="h-full w-full"></div>
