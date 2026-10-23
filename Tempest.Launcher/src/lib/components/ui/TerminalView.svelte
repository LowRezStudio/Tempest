<script lang="ts">
	import { onMount, tick } from "svelte";
	import CommandInput from "$lib/components/ui/CommandInput.svelte";
	import { openTextContextMenu } from "$lib/platform/context-menu";
	import { Scrollback, type Line } from "$lib/utils/scrollback";
	import type { Child } from "$lib/platform/api";
	import type { ProcessLog } from "$lib/types/process";

	interface Props {
		logs: ReadonlyArray<ProcessLog>;
		/** Process whose stdin receives submitted commands (shows the input bar). */
		child?: Child | null;
		showPrefix?: boolean;
	}

	let { logs, child = null, showPrefix = true }: Props = $props();

	/** Slack below the last line that still counts as "following the output". */
	const PIN_SLACK = 32;
	const PREFIX_CLS = "ansi-fg-0";

	let viewport = $state<HTMLDivElement>();
	let viewLines = $state.raw<Line[]>([]);

	const scrollback = new Scrollback();
	let pinned = true;
	let frame = 0;

	function prefixOf(source: string): string {
		return showPrefix && source ? `[${source}] ` : "";
	}

	async function scrollToBottom(): Promise<void> {
		await tick();
		const element = viewport;
		if (!element) return;
		element.scrollTop = element.scrollHeight;
		pinned = true;
		// Offscreen lines are laid out lazily, so the real height settles a frame later.
		requestAnimationFrame(() => {
			if (pinned) element.scrollTop = element.scrollHeight;
		});
	}

	function flush(): void {
		frame = 0;
		viewLines = scrollback.toArray();
		if (pinned) void scrollToBottom();
	}

	// Coalesce bursts of log lines into one DOM update per frame.
	function requestFrame(): void {
		if (frame === 0) frame = requestAnimationFrame(flush);
	}

	function handleScroll(): void {
		const element = viewport;
		if (!element) return;
		pinned = element.scrollHeight - element.scrollTop - element.clientHeight <= PIN_SLACK;
	}

	async function handleContextMenu(event: MouseEvent): Promise<void> {
		const action = await openTextContextMenu(event, { canSelectAll: viewLines.length > 0 });
		if (!action || !viewport) return;

		const selection = window.getSelection();
		if (!selection) return;
		if (action === "selectAll") selection.selectAllChildren(viewport);
		if (selection.rangeCount === 0) return;

		try {
			await navigator.clipboard.writeText(selection.toString());
		} catch (error) {
			console.error("Failed to copy logs:", error);
		}
	}

	function echoCommand(line: string): void {
		scrollback.echo(line);
		requestFrame();
	}

	$effect(() => {
		// A stream that is gone (cleared or filtered) restarts the follow state.
		if (scrollback.sync(logs)) pinned = true;
		requestFrame();
	});

	onMount(() => () => cancelAnimationFrame(frame));
</script>

<div class="bg-base-300 flex h-full min-h-0 cursor-text flex-col font-mono text-[12px]">
	<div
		bind:this={viewport}
		onscroll={handleScroll}
		oncontextmenu={handleContextMenu}
		tabindex="-1"
		class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain px-3 py-1.5 select-text"
		role="log"
		aria-live="off"
	>
		{#each viewLines as line (line.id)}
			{@const prefix = prefixOf(line.source)}
			<div
				class="terminal-line"
				class:text-error={line.error}
				class:text-primary={line.input}
			>
				<!-- No whitespace between the blocks: pre-wrap would render it as a leading space. -->
				{#if prefix}<span class={PREFIX_CLS}>{prefix}</span
					>{/if}{#each line.runs as run}<span class={run.cls}>{run.text}</span>{/each}
			</div>
		{/each}
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
	.terminal-line {
		line-height: 17px;
		white-space: pre-wrap;
		/* Fill every row to the last column like a terminal; break-all still keeps punctuation attached. */
		line-break: anywhere;
		/* Skip layout and paint for lines far outside the viewport. */
		content-visibility: auto;
		contain-intrinsic-size: auto 17px;
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
