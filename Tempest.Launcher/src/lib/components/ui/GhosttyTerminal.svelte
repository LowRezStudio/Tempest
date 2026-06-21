<script lang="ts">
	import { onDestroy, onMount } from "svelte";
	import type { Child } from "@tauri-apps/plugin-shell";
	import type { ProcessLog } from "$lib/types/process";

	interface Props {
		logs: ReadonlyArray<ProcessLog>;
		child?: Child | null;
	}

	let { logs, child }: Props = $props();

	let container = $state<HTMLDivElement>();
	let term = $state<import("ghostty-web").Terminal | undefined>();
	let lastId = -1;
	let pendingRaf: number | null = null;
	const encoder = new TextEncoder();

	let resizeObserver = $state<ResizeObserver | undefined>();
	let dataDisposable: { dispose: () => void } | undefined;

	onMount(async () => {
		const { Ghostty, Terminal } = await import("ghostty-web");
		const wasmUrl = (await import("ghostty-web/ghostty-vt.wasm?url")).default;
		const ghostty = await Ghostty.load(wasmUrl);

		term = new Terminal({
			ghostty,
			fontSize: 13,
			fontFamily: "'Ubuntu Sans Mono Variable', monospace",
			theme: {
				background: "#1e1e2e",
				foreground: "#cdd6f4",
			},
			scrollback: 10000,
			disableStdin: !child,
		});

		if (!container) return;

		term.open(container);
		if (term.element) {
			term.element.style.width = "100%";
			term.element.style.height = "100%";
		}
		term.write("\x1b[?25l");

		if (child) {
			dataDisposable = term.onData((data) => {
				if (child) {
					void child.write(data);
				}
			});
		}

		await document.fonts.ready;
		fit(term, container);

		resizeObserver = new ResizeObserver(() => {
			if (term && container) fit(term, container);
		});
		resizeObserver.observe(container);
	});

	onDestroy(() => {
		resizeObserver?.disconnect();
		dataDisposable?.dispose();
		if (pendingRaf !== null) {
			cancelAnimationFrame(pendingRaf);
		}
		term?.dispose();
	});

	$effect(() => {
		void logs.length;
		if (!term) return;
		scheduleFlush();
	});

	function fit(terminal: import("ghostty-web").Terminal, parent: HTMLDivElement): void {
		const renderer = (
			terminal as unknown as {
				renderer?: { getMetrics: () => { width: number; height: number } };
			}
		).renderer;
		if (!renderer) return;

		const metrics = renderer.getMetrics();
		if (!metrics.width || !metrics.height) return;

		const cols = Math.max(1, Math.floor(parent.clientWidth / metrics.width));
		const rows = Math.max(1, Math.floor(parent.clientHeight / metrics.height));
		terminal.resize(cols, rows);
	}

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
			term.write(encoder.encode(chunk));
			lastId = nextId;
		}
	}
</script>

<div bind:this={container} class="h-full w-full"></div>
