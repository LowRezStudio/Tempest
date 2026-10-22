<script lang="ts">
	import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
	import {
		bracketMatching,
		indentOnInput,
		syntaxHighlighting,
		defaultHighlightStyle,
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
	import { Hammer, RotateCcw } from "@lucide/svelte";
	import { onDestroy } from "svelte";
	import { compileFailure, rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import { unrealscript } from "$lib/studio/unrealscript";
	import type { CompileFailure, ScriptSource } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let host = $state<HTMLDivElement | null>(null);
	let info = $state<ScriptSource | null>(null);
	let error = $state<string | null>(null);
	let failure = $state<CompileFailure | null>(null);
	let warnings = $state<string[]>([]);
	let compiling = $state(false);
	let modified = $state(false);
	let view: EditorView | null = null;
	const themeCompartment = new Compartment();

	const draftKey = $derived(info ? `${path}|${info.class}` : null);

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
					unrealscript,
					themeCompartment.of(themeExt()),
					EditorState.tabSize.of(4),
					keymap.of([
						{ key: "Mod-Enter", run: () => (void compile(), true) },
						indentWithTab,
						...defaultKeymap,
						...historyKeymap,
						...searchKeymap,
					]),
					EditorView.updateListener.of((u) => {
						if (u.docChanged && draftKey) {
							const text = u.state.doc.toString();
							modified = text !== info?.source;
							if (modified) studio.drafts[draftKey] = text;
							else delete studio.drafts[draftKey];
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

	/** Line of the member the user selected (function, state, …). */
	function findMember(text: string, member: string): number | null {
		const re = new RegExp(
			`^[^/\\n]*\\b(function|event|state|operator|preoperator|postoperator|delegate|struct|enum|const)\\b[^\\n]*\\b${member.replaceAll(/[.*+?^${}()|[\\]\\\\]/g, "\\\\$&")}\\b`,
			"im",
		);
		const m = re.exec(text);
		if (!m) return null;
		return text.slice(0, m.index).split("\n").length;
	}

	async function load() {
		error = null;
		failure = null;
		warnings = [];
		try {
			const r = await studio.run("Decompiling…", () => rpc("script.source", { path, index }));
			info = r;
			const key = `${path}|${r.class}`;
			const draft = studio.drafts[key];
			const text = draft ?? r.source;
			modified = draft !== undefined;
			mount(text);
			if (r.member) {
				const line = findMember(text, r.member);
				if (line) requestAnimationFrame(() => goToLine(line));
			}
		} catch (error_) {
			error = error_ instanceof Error ? error_.message : String(error_);
		}
	}

	$effect(() => {
		void [path, index];
		void load();
	});

	$effect(() => {
		void studio.theme;
		view?.dispatch({ effects: themeCompartment.reconfigure(themeExt()) });
	});

	async function compile() {
		if (!view || !info || compiling) return;
		const source = view.state.doc.toString();
		compiling = true;
		failure = null;
		warnings = [];
		try {
			const r = await studio.run(`Compiling ${studio.pkg?.name ?? ""}…`, () =>
				rpc("script.compile", { path, class: info!.class, source }),
			);
			warnings = r.warnings;
			info = { ...info, source };
			modified = false;
			if (draftKey) delete studio.drafts[draftKey];
			studio.markDirty(path);
			// The package was regenerated: refresh the tables and keep the selection.
			const tables = r;
			tables.path = path;
			studio.pkg = tables;
			studio.tablesVersion++;
			toasts.push(
				"success",
				`${r.exports.length} exports rebuilt in ${r.compileMs} ms${r.warnings.length ? `, ${r.warnings.length} warnings` : ""}. Save the package to write it.`,
				"Compiled",
			);
		} catch (error_) {
			failure = compileFailure(error_);
			if (
				failure.line &&
				(!failure.class || failure.class.toLowerCase() === info.class.toLowerCase())
			) {
				goToLine(failure.line);
			}
		} finally {
			compiling = false;
		}
	}

	function revert() {
		if (!view || !info) return;
		view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: info.source } });
	}

	onDestroy(() => view?.destroy());
</script>

<div class="flex h-full flex-col">
	<div class="border-base-300 flex flex-wrap items-center gap-2 border-b px-3 py-1.5 text-xs">
		{#if info}
			<span class="font-mono font-semibold">{info.class}.uc</span>
			{#if info.member}<span class="font-mono opacity-60">· {info.kind} {info.member}</span
				>{/if}
			{#if modified}<span class="badge badge-sm badge-warning">modified</span>{/if}
			<div class="flex-1"></div>
			<button
				class="btn btn-ghost btn-xs"
				onclick={revert}
				disabled={!modified}
				title="Back to the decompiled text"
			>
				<RotateCcw size={12} /> Revert
			</button>
			<button
				class="btn btn-xs btn-primary"
				onclick={compile}
				disabled={compiling || studio.busy !== null}
				title="Recompile the package with this class (Ctrl+Enter)"
			>
				{#if compiling}<span class="loading loading-xs loading-spinner"
					></span>{:else}<Hammer size={12} />{/if}
				Compile
			</button>
		{/if}
	</div>
	{#if error}
		<div class="text-error p-3 text-sm">{error}</div>
	{:else if studio.isDirty && !modified && info}
		<div class="border-base-300 bg-warning/10 border-b px-3 py-1 text-[11px]">
			The package holds unsaved changes. Recompiling needs a saved package: save first, then
			compile again.
		</div>
	{/if}
	<div class="relative min-h-0 flex-1 overflow-hidden" bind:this={host}></div>
	{#if failure || warnings.length}
		<div
			class="border-base-300 max-h-32 shrink-0 overflow-auto border-t px-3 py-1 font-mono text-[11px]"
		>
			{#if failure}
				<button
					class="text-error text-left hover:underline"
					onclick={() => failure?.line && goToLine(failure.line)}
				>
					{#if failure.line}Line {failure.line}: {failure.detail ??
							failure.message}{:else}{failure.message}{/if}
				</button>
				{#if failure.class && info && failure.class.toLowerCase() !== info.class.toLowerCase()}
					<div class="opacity-70">
						in class {failure.class} (another class of the package depends on this one)
					</div>
				{/if}
			{/if}
			{#each warnings as w, k (k)}
				<div class="text-warning">{w}</div>
			{/each}
		</div>
	{/if}
</div>
