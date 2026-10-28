<!--
	One script of a movie as decompiled ActionScript 2 or as P-code
	(`swf.script`, with the movie's pending edits applied). Check compiles
	without writing (`swf.compile`); Stage keeps the text as a pending edit of
	the movie (`script` / `pcode`), which the editor's Apply writes into the
	package with the other edits. Switching between the two views stages the
	text first, so the other view shows what it compiles to.
-->
<script lang="ts">
	import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
	import {
		bracketMatching,
		defaultHighlightStyle,
		indentOnInput,
		syntaxHighlighting,
	} from "@codemirror/language";
	import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
	import { Compartment, EditorState } from "@codemirror/state";
	import { oneDark } from "@codemirror/theme-one-dark";
	import {
		drawSelection,
		EditorView,
		highlightActiveLine,
		keymap,
		lineNumbers,
	} from "@codemirror/view";
	import { CheckCheck, RotateCcw, Upload } from "@lucide/svelte";
	import { onDestroy } from "svelte";
	import { actionscript, errorLine, pcode } from "$lib/studio/as2";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { SwfDoc } from "$lib/studio/swf.svelte";
	import type { SwfScript } from "../../../../electron/src/ipc/contract";

	interface Props {
		doc: SwfDoc;
		script: SwfScript;
		title: string;
	}
	const { doc, script, title }: Props = $props();

	type Mode = "source" | "pcode";

	let host = $state<HTMLDivElement | null>(null);
	let mode = $state<Mode>("source");
	let loaded = $state<{ pcode: string; source: string; problems: string[] } | null>(null);
	let loadError = $state<string | null>(null);
	let failure = $state<{ line: number | null; message: string } | null>(null);
	let checked = $state<string | null>(null);
	let busy = $state(false);
	let modified = $state(false);
	/** The text the editor started from: a modification is a difference from it. */
	let base = "";
	let view: EditorView | null = null;
	const theme = new Compartment();
	const language = new Compartment();

	const pending = $derived(doc.scriptEdit(script.loc));

	function themeExt() {
		return studio.theme === "mocha" ? oneDark : syntaxHighlighting(defaultHighlightStyle);
	}

	function mount(text: string) {
		if (!host) return;
		view?.destroy();
		view = new EditorView({
			parent: host,
			state: EditorState.create({
				doc: text,
				extensions: [
					lineNumbers(),
					history(),
					drawSelection(),
					highlightActiveLine(),
					bracketMatching(),
					indentOnInput(),
					highlightSelectionMatches(),
					language.of(mode === "source" ? actionscript : pcode),
					theme.of(themeExt()),
					EditorState.tabSize.of(4),
					keymap.of([
						{ key: "Mod-Enter", run: () => (void stage(), true) },
						indentWithTab,
						...defaultKeymap,
						...historyKeymap,
						...searchKeymap,
					]),
					EditorView.updateListener.of((u) => {
						if (u.docChanged) {
							modified = u.state.doc.toString() !== base;
							checked = null;
						}
					}),
					EditorView.theme({
						"&": { height: "100%" },
						".cm-scroller": { fontFamily: "var(--font-mono)", fontSize: "12px" },
					}),
				],
			}),
		});
	}

	/** Read the script (pending edits included) and show it in the current view. */
	async function load() {
		loadError = null;
		failure = null;
		checked = null;
		try {
			const r = await rpc("swf.script", {
				path: doc.path,
				index: doc.index,
				loc: $state.snapshot(script.loc),
				edits: doc.pending,
			});
			loaded = r;
			const p = doc.scriptEdit(script.loc);
			// The text the user staged in this view stays as written (comments, layout).
			const own =
				p?.kind === "script" && mode === "source"
					? p.source
					: p?.kind === "pcode" && mode === "pcode"
						? p.pcode
						: null;
			base = own ?? (mode === "source" ? r.source : r.pcode);
			modified = false;
			mount(base);
		} catch (error) {
			loadError = error instanceof Error ? error.message : String(error);
		}
	}

	// A new script or view mounts a fresh editor.
	$effect(() => {
		void script.key;
		void mode;
		void load();
	});

	$effect(() => {
		void studio.theme;
		view?.dispatch({ effects: theme.reconfigure(themeExt()) });
	});

	onDestroy(() => view?.destroy());

	function goToLine(line: number) {
		if (!view) return;
		const n = Math.max(1, Math.min(line, view.state.doc.lines));
		const pos = view.state.doc.line(n).from;
		view.dispatch({
			selection: { anchor: pos },
			effects: EditorView.scrollIntoView(pos, { y: "center" }),
		});
		view.focus();
	}

	/** Compile the editor's text; true when it compiles. */
	async function check(): Promise<boolean> {
		if (!view) return false;
		const text = view.state.doc.toString();
		failure = null;
		try {
			const r = await rpc(
				"swf.compile",
				mode === "source" ? { source: text } : { pcode: text },
			);
			checked = `${r.bytes} bytes`;
			return true;
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			failure = { line: errorLine(message), message };
			if (failure.line) goToLine(failure.line);
			return false;
		}
	}

	/** Keep the text as the script's pending replacement. */
	async function stage(): Promise<boolean> {
		if (!view || busy) return false;
		busy = true;
		try {
			if (!(await check())) return false;
			const text = view.state.doc.toString();
			doc.setScript($state.snapshot(script.loc), text, mode);
			base = text;
			modified = false;
			return true;
		} finally {
			busy = false;
		}
	}

	async function switchTo(m: Mode) {
		if (m === mode) return;
		if (modified && !(await stage())) {
			toasts.push("warning", "Fix the error first, or revert the script.", "Not compiled");
			return;
		}
		await doc.settled();
		mode = m;
	}

	function revert() {
		doc.revertScript($state.snapshot(script.loc));
		void doc.settled().then(load);
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="border-base-300 flex flex-wrap items-center gap-2 border-b px-2 py-1 text-xs">
		<span class="truncate font-mono font-semibold" title={script.key}>{title}</span>
		<span class="opacity-50">{script.kind} · {script.bytes} bytes</span>
		{#if pending}<span
				class="badge badge-sm badge-warning"
				title="Apply writes it into the package">staged</span
			>{/if}
		{#if modified}<span class="badge badge-sm badge-info">modified</span>{/if}
		<div class="flex-1"></div>
		<div class="join">
			<button
				class="btn join-item btn-xs {mode === 'source' ? 'btn-active' : ''}"
				onclick={() => switchTo("source")}>ActionScript</button
			>
			<button
				class="btn join-item btn-xs {mode === 'pcode' ? 'btn-active' : ''}"
				onclick={() => switchTo("pcode")}>P-code</button
			>
		</div>
		<button
			class="btn btn-ghost btn-xs"
			onclick={revert}
			disabled={!pending && !modified}
			title="Back to the script as it is in the movie"
		>
			<RotateCcw size={12} /> Revert
		</button>
		<button class="btn btn-ghost btn-xs" onclick={check} disabled={busy} title="Compile only">
			<CheckCheck size={12} /> Check
		</button>
		<button
			class="btn btn-xs btn-primary"
			onclick={stage}
			disabled={busy || !modified}
			title="Keep it as a pending edit of the movie; Apply writes it (Ctrl+Enter)"
		>
			<Upload size={12} /> Stage
		</button>
	</div>
	{#if loadError}
		<div class="text-error p-3 text-sm">{loadError}</div>
	{:else if loaded && loaded.problems.length > 0 && mode === "source"}
		<details class="border-base-300 bg-warning/10 border-b px-2 py-1 text-[11px]">
			<summary class="cursor-pointer">
				{loaded.problems.length} part{loaded.problems.length > 1 ? "s" : ""} kept as P-code (<code
					>__pcode</code
				> blocks compile as written)
			</summary>
			{#each loaded.problems as p, k (k)}<div class="font-mono opacity-70">{p}</div>{/each}
		</details>
	{/if}
	<div class="relative min-h-0 flex-1 overflow-hidden" bind:this={host}></div>
	{#if failure || checked}
		<div class="border-base-300 shrink-0 border-t px-2 py-1 font-mono text-[11px]">
			{#if failure}
				<button
					class="text-error text-left hover:underline"
					onclick={() => failure?.line && goToLine(failure.line)}
					>{failure.message}</button
				>
			{:else}
				<span class="text-success">Compiles: {checked}</span>
			{/if}
		</div>
	{/if}
</div>
