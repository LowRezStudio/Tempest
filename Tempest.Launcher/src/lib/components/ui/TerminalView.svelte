<script lang="ts">
	import { onMount, tick } from "svelte";
	import CommandInput from "$lib/components/ui/CommandInput.svelte";
	import { openTextContextMenu } from "$lib/platform/context-menu";
	import { Scrollback, type Row } from "$lib/utils/scrollback";
	import { unwrappedSelection } from "$lib/utils/selection";
	import type { Child } from "$lib/platform/api";
	import type { ProcessLog } from "$lib/types/process";

	interface Props {
		logs: ReadonlyArray<ProcessLog>;
		/** Process whose stdin receives submitted commands (shows the input bar). */
		child?: Child | null;
		showPrefix?: boolean;
	}

	let { logs, child = null, showPrefix = true }: Props = $props();

	const ROW_HEIGHT = 17;
	const GUTTER = 6;
	const OVERSCAN = 10;
	const DEFAULT_COLUMNS = 100;
	const REWRAP_DELAY = 100;
	const PREFIX_CLS = "ansi-fg-0";

	let viewport = $state<HTMLDivElement>();
	let charProbe = $state<HTMLSpanElement>();
	let viewRows = $state.raw<(Row & { index: number })[]>([]);
	let padTop = $state(GUTTER);
	let padBottom = $state(GUTTER);
	let selectAll = $state(false);

	const scrollback = new Scrollback();
	let scrollTop = 0;
	let viewportHeight = 0;
	let columns = DEFAULT_COLUMNS;
	let wrappedPrefix: boolean | null = null;
	let pinned = true;
	let frame = 0;
	let rewrapTimer: ReturnType<typeof setTimeout> | undefined;

	function prefixOf(source: string): string {
		return showPrefix && source ? `[${source}] ` : "";
	}

	async function scrollToBottom(): Promise<void> {
		await tick();
		const element = viewport;
		if (!element) return;
		element.scrollTop = element.scrollHeight;
		scrollTop = element.scrollTop;
		pinned = true;
		renderWindow();
	}

	// Update the column count from the measured monospace advance.
	function measureColumns(): boolean {
		const element = viewport;
		const probe = charProbe;
		if (!element || !probe) return false;

		const charWidth = probe.getBoundingClientRect().width;
		if (charWidth <= 0) return false;

		const style = getComputedStyle(element);
		const paddingLeft = Number.parseFloat(style.paddingLeft) || 0;
		const paddingRight = Number.parseFloat(style.paddingRight) || 0;
		const available = element.clientWidth - paddingLeft - paddingRight;
		const next = Math.max(10, Math.floor(available / charWidth));
		if (next === columns) return false;

		columns = next;
		return true;
	}

	// Rebuilds only the rows the viewport can see; everything else is a spacer.
	function renderWindow(): void {
		const total = scrollback.length;
		const firstVisible = Math.min(total - 1, Math.floor((scrollTop - GUTTER) / ROW_HEIGHT));
		const first = Math.max(0, firstVisible - OVERSCAN);
		const visible = Math.ceil(viewportHeight / ROW_HEIGHT) + OVERSCAN * 2 + 1;
		const last = Math.min(total, first + visible);
		const rows: (Row & { index: number })[] = [];

		for (let index = first; index < last; index++) {
			const row = scrollback.at(index)!;
			const prefix = row.first ? prefixOf(row.source) : "";
			const runs = prefix ? [{ text: prefix, cls: PREFIX_CLS }, ...row.runs] : row.runs;
			rows.push({ ...row, index, runs });
		}

		viewRows = rows;
		padTop = GUTTER + first * ROW_HEIGHT;
		padBottom = GUTTER + (total - last) * ROW_HEIGHT;
	}

	function flush(): void {
		frame = 0;
		renderWindow();
		if (pinned) void scrollToBottom();
	}

	function scheduleRewrap(): void {
		clearTimeout(rewrapTimer);
		rewrapTimer = setTimeout(() => {
			rewrapTimer = undefined;
			scrollback.rewrap(columns, prefixOf);
			renderWindow();
			if (pinned) void scrollToBottom();
		}, REWRAP_DELAY);
	}

	function handleScroll(): void {
		const element = viewport;
		if (!element) return;
		scrollTop = element.scrollTop;
		pinned = element.scrollHeight - scrollTop - element.clientHeight <= ROW_HEIGHT * 2;
		renderWindow();
	}

	function handlePointerDown(event: PointerEvent): void {
		// Starting a mouse selection replaces the log-model selection.
		if (event.button === 0) {
			selectAll = false;
			viewport?.focus({ preventScroll: true });
		}
	}

	function selectedText(): string {
		if (selectAll) return scrollback.text(prefixOf);
		const selection = window.getSelection();
		if (!selection?.rangeCount) return "";
		return unwrappedSelection(selection.getRangeAt(0).cloneContents());
	}

	function handleCopy(event: ClipboardEvent): void {
		if (!event.clipboardData) return;
		const text = selectedText();
		if (!text) return;
		event.preventDefault();
		event.clipboardData.setData("text/plain", text);
	}

	async function copySelection(): Promise<void> {
		const text = selectedText();
		if (text.length === 0) return;
		try {
			await navigator.clipboard.writeText(text);
		} catch (error) {
			console.error("Failed to copy logs:", error);
		}
	}

	async function handleContextMenu(event: MouseEvent): Promise<void> {
		const action = await openTextContextMenu(event, {
			canCopy: selectAll || (window.getSelection()?.toString().length ?? 0) > 0,
			canSelectAll: scrollback.length > 0,
		});
		if (!action) return;

		if (action === "selectAll") {
			window.getSelection()?.removeAllRanges();
			selectAll = true;
			viewport?.focus({ preventScroll: true });
		}
		await copySelection();
	}

	function echoCommand(line: string): void {
		scrollback.echo(line, columns);
		renderWindow();
		if (pinned) void scrollToBottom();
	}

	$effect(() => {
		// The prefix changes the first column budget, so the retained rows re-wrap.
		if (wrappedPrefix !== showPrefix) {
			wrappedPrefix = showPrefix;
			scrollback.rewrap(columns, prefixOf);
		}

		const { dropped, restarted } = scrollback.sync(logs, columns, prefixOf);

		if (restarted) {
			// The retained rows belong to a stream that is gone (cleared or filtered).
			selectAll = false;
			scrollTop = 0;
			pinned = true;
			if (viewport) viewport.scrollTop = 0;
		} else if (dropped > 0 && !pinned && viewport) {
			// Compensate for evicted rows so the text being read stays under the cursor.
			viewport.scrollTop -= dropped * ROW_HEIGHT;
			scrollTop = viewport.scrollTop;
		}

		// Coalesce bursts of log lines into one DOM update per frame.
		if (frame === 0) frame = requestAnimationFrame(flush);
	});

	onMount(() => {
		const element = viewport;
		if (!element) return;

		if (measureColumns()) scheduleRewrap();
		viewportHeight = element.clientHeight;
		scrollTop = element.scrollTop;
		void scrollToBottom();

		const observer = new ResizeObserver(() => {
			viewportHeight = element.clientHeight;
			renderWindow();
			if (measureColumns()) scheduleRewrap();
			if (pinned) void scrollToBottom();
		});
		observer.observe(element);

		let active = true;
		void document.fonts.ready.then(() => {
			if (active && measureColumns()) scheduleRewrap();
		});

		return () => {
			active = false;
			observer.disconnect();
			clearTimeout(rewrapTimer);
			cancelAnimationFrame(frame);
		};
	});
</script>

<div class="bg-base-300 flex h-full min-h-0 flex-col font-mono text-[12px]">
	<span bind:this={charProbe} class="terminal-char" aria-hidden="true">0</span>
	<div
		bind:this={viewport}
		onscroll={handleScroll}
		onpointerdown={handlePointerDown}
		oncontextmenu={handleContextMenu}
		oncopy={handleCopy}
		tabindex="-1"
		class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain px-3 select-text"
		role="log"
		aria-live="off"
	>
		<div style="height: {padTop}px"></div>
		{#each viewRows as item (item.index)}
			<div
				class="terminal-row"
				data-first={item.first}
				class:text-error={item.error}
				class:terminal-row-input={item.input}
				class:terminal-row-selected={selectAll}
				style="height: {ROW_HEIGHT}px; line-height: {ROW_HEIGHT}px"
			>
				{#each item.runs as run}<span class={run.cls}>{run.text}</span>{/each}
			</div>
		{/each}
		<div style="height: {padBottom}px"></div>
	</div>

	{#if child}
		<CommandInput
			{child}
			onsend={echoCommand}
			class="border-base-content/10 shrink-0 border-t px-3 py-2"
		/>
	{/if}
</div>

<style>
	.terminal-char {
		position: absolute;
		top: 0;
		left: -9999px;
		display: block;
		width: 1ch;
		height: 0;
		visibility: hidden;
		pointer-events: none;
	}

	.terminal-row {
		box-sizing: border-box;
		width: 100%;
		white-space: pre;
	}

	.terminal-row-input {
		color: var(--color-primary);
	}

	.terminal-row-selected {
		background: color-mix(in oklab, var(--color-primary) 22%, transparent);
	}

	/* The ANSI palette follows the active theme, with bright colors reusing normal slots. */
	:global(.ansi-fg-0) {
		color: color-mix(in oklab, var(--color-base-content) 60%, transparent);
	}
	:global(.ansi-fg-1) {
		color: var(--color-error);
	}
	:global(.ansi-fg-2) {
		color: var(--color-success);
	}
	:global(.ansi-fg-3) {
		color: var(--color-warning);
	}
	:global(.ansi-fg-4) {
		color: var(--color-primary);
	}
	:global(.ansi-fg-5) {
		color: color-mix(in oklab, var(--color-primary) 55%, var(--color-error));
	}
	:global(.ansi-fg-6) {
		color: var(--color-info);
	}
	:global(.ansi-fg-7) {
		color: var(--color-base-content);
	}
	:global(.ansi-bold) {
		font-weight: 600;
	}
	:global(.ansi-dim) {
		opacity: 0.6;
	}
	:global(.ansi-italic) {
		font-style: italic;
	}
	:global(.ansi-underline) {
		text-decoration: underline;
	}
	:global(.ansi-strike) {
		text-decoration: line-through;
	}
</style>
