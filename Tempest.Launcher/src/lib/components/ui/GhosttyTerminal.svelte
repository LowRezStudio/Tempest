<script lang="ts">
	import { FitAddon, Ghostty, Terminal } from "ghostty-web";
	import { onDestroy, onMount } from "svelte";
	import type { Child } from "@tauri-apps/plugin-shell";
	import type { ProcessLog } from "$lib/types/process";

	interface Props {
		logs: ReadonlyArray<ProcessLog>;
		child?: Child | null;
		showPrefix?: boolean;
	}

	let { logs, child, showPrefix = true }: Props = $props();

	let container = $state<HTMLDivElement>();
	let term = $state<Terminal | undefined>();
	let lastId = -1;
	let pendingRaf: number | null = null;

	let fitAddon: FitAddon | undefined;
	let dataDisposable: { dispose: () => void } | undefined;

	function resolveColors(
		el: HTMLDivElement | undefined,
		mapping: Record<string, { cssVar: string; fallback: string }>
	): Record<string, string> {
		const result: Record<string, string> = {};
		if (!el) {
			for (const [key, val] of Object.entries(mapping)) {
				result[key] = val.fallback;
			}
			return result;
		}

		const computed = getComputedStyle(el);
		const ctx = document.createElement("canvas").getContext("2d");

		for (const [key, val] of Object.entries(mapping)) {
			const styleColor = computed.getPropertyValue(val.cssVar).trim();
			if (!styleColor) {
				result[key] = val.fallback;
				continue;
			}
			if (!ctx) {
				result[key] = styleColor;
				continue;
			}
			ctx.fillStyle = styleColor;
			const resolved = ctx.fillStyle;
			result[key] = resolved.startsWith("#") ? resolved.slice(0, 7) : resolved;
		}
		return result;
	}

	onMount(async () => {
		const wasmUrl = (await import("ghostty-web/ghostty-vt.wasm?url")).default;
		const ghostty = await Ghostty.load(wasmUrl);

		if (!container) return;

		const colors = resolveColors(container, {
			bg: { cssVar: "--color-base-300", fallback: "#1e1e2e" },
			fg: { cssVar: "--color-base-content", fallback: "#cdd6f4" },
		});

		term = new Terminal({
			ghostty,
			fontSize: 13,
			fontFamily: "'Ubuntu Sans Mono Variable', monospace",
			theme: {
				background: colors.bg,
				foreground: colors.fg,
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

		let startIndex = 0;
		if (lastId !== -1) {
			let low = 0;
			let high = current.length - 1;
			startIndex = current.length; // Default to end of array if none found
			while (low <= high) {
				const mid = (low + high) >> 1;
				if (current[mid].id > lastId) {
					startIndex = mid;
					high = mid - 1;
				} else {
					low = mid + 1;
				}
			}
		}

		let chunk = "";
		let nextId = lastId;
		for (let i = startIndex; i < current.length; i++) {
			const log = current[i];
			const prefix = showPrefix && log.source ? `\x1b[90m[${log.source}]\x1b[0m ` : "";
			const color = log.error ? "\x1b[31m" : "";
			const line = log.line.replaceAll(/\r?\n/g, "\r\n");
			chunk += `${prefix}${color}${line}\x1b[0m\r\n`;
			nextId = log.id;
		}

		if (chunk) {
			const wasAtBottom = term.getViewportY() < 1;
			term.write(chunk);
			lastId = nextId;
			if (wasAtBottom) {
				term.scrollToBottom();
			}
		}
	}
</script>

<div bind:this={container} class="h-full w-full"></div>
