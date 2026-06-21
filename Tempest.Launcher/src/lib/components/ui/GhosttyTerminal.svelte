<script lang="ts">
	import { FitAddon, Ghostty, Terminal } from "ghostty-web";
	import { onDestroy, onMount } from "svelte";
	import type { Child } from "@tauri-apps/plugin-shell";
	import type { ProcessLog } from "$lib/types/process";

	interface Props {
		logs: ReadonlyArray<ProcessLog>;
		child?: Child | null;
	}

	let { logs, child }: Props = $props();

	let container = $state<HTMLDivElement>();
	let term = $state<Terminal | undefined>();
	let lastId = -1;
	let pendingRaf: number | null = null;
	const encoder = new TextEncoder();

	let fitAddon: FitAddon | undefined;
	let dataDisposable: { dispose: () => void } | undefined;

	function resolveColor(cssVar: string, fallback: string): string {
		if (!container) return fallback;
		const styleColor = getComputedStyle(container).getPropertyValue(cssVar).trim();
		if (!styleColor) return fallback;

		const ctx = document.createElement("canvas").getContext("2d");
		if (!ctx) return styleColor;
		ctx.fillStyle = styleColor;
		const resolved = ctx.fillStyle;
		if (resolved.startsWith("#")) {
			return resolved.slice(0, 7);
		}
		return resolved;
	}

	onMount(async () => {
		const wasmUrl = (await import("ghostty-web/ghostty-vt.wasm?url")).default;
		const ghostty = await Ghostty.load(wasmUrl);

		if (!container) return;

		const bg = resolveColor("--color-base-300", "#1e1e2e");
		const fg = resolveColor("--color-base-content", "#cdd6f4");

		term = new Terminal({
			ghostty,
			fontSize: 13,
			fontFamily: "'Ubuntu Sans Mono Variable', monospace",
			theme: {
				background: bg,
				foreground: fg,
			},
			scrollback: 10000,
			disableStdin: false,
		});

		term.open(container);
		if (term.element) {
			term.element.style.width = "100%";
			term.element.style.height = "100%";
		}
		term.write("\x1b[?25l");

		dataDisposable = term.onData((data) => {
			if (child) {
				void child.write(data);
			}
		});

		await document.fonts.ready;

		fitAddon = new FitAddon();
		term.loadAddon(fitAddon);
		fitAddon.fit();
		fitAddon.observeResize();
	});

	onDestroy(() => {
		fitAddon?.dispose();
		dataDisposable?.dispose();
		if (pendingRaf !== null) {
			cancelAnimationFrame(pendingRaf);
		}
		term?.dispose();
	});

	$effect(() => {
		void logs;
		if (!term) return;
		scheduleFlush();
	});

	function scheduleFlush(): void {
		if (pendingRaf !== null) return;
		pendingRaf = requestAnimationFrame(flush);
	}

	function flush(): void {
		pendingRaf = null;
		if (!term) return;

		const current = logs;
		if (current.length === 0) {
			term.clear();
			lastId = -1;
			return;
		}

		if (current[0].id > lastId && lastId !== -1) {
			term.clear();
			lastId = -1;
		}

		let chunk = "";
		let nextId = lastId;
		for (const log of current) {
			if (log.id <= lastId) continue;
			const prefix = log.source ? `\x1b[90m[${log.source}]\x1b[0m ` : "";
			const color = log.error ? "\x1b[31m" : "";
			const line = log.line.replaceAll(/\r?\n/g, "\r\n");
			chunk += `${prefix}${color}${line}\x1b[0m\r\n`;
			nextId = log.id;
		}

		if (chunk) {
			const wasAtBottom = term.getViewportY() < 1;
			term.write(encoder.encode(chunk));
			lastId = nextId;
			if (wasAtBottom) {
				term.scrollToBottom();
			}
		}
	}
</script>

<div bind:this={container} class="h-full w-full"></div>
