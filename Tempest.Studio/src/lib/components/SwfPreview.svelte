<!--
	The movie editor on SwfMovie exports, laid out like FFDec: the resource
	tree (`swf/SwfTree`), the stage with the timeline of the open sprite or
	the root under it (`swf/SwfStage`, `swf/SwfTimeline`), and the
	properties of the selected object or character (`swf/SwfProps`). The
	"Movie" node plays the whole movie (`SwfPlayer`); the tag views
	(`swf/SwfSections`) list texts, scripts, images, linkage and tags.
	Edits are pending until Apply writes them into the package as one undo
	step; the state lives in `$lib/studio/swf.svelte.ts`.
-->
<script lang="ts">
	import {
		Check,
		ChevronRight,
		Maximize,
		Move,
		PackagePlus,
		PanelLeftClose,
		PanelLeftOpen,
		Play,
		RotateCcw,
		Scaling,
		Scan,
		Square,
		ZoomIn,
		ZoomOut,
	} from "@lucide/svelte";
	import { untrack } from "svelte";
	import Pane from "$lib/components/shell/Pane.svelte";
	import SwfProps from "$lib/components/swf/SwfProps.svelte";
	import SwfSections from "$lib/components/swf/SwfSections.svelte";
	import SwfStage from "$lib/components/swf/SwfStage.svelte";
	import SwfTimeline from "$lib/components/swf/SwfTimeline.svelte";
	import SwfTree from "$lib/components/swf/SwfTree.svelte";
	import SwfPlayer from "$lib/components/SwfPlayer.svelte";
	import { cooker } from "$lib/studio/cooker.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { swfDoc } from "$lib/studio/swf.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { SwfPreviewTarget, SwfRect } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	const doc = $derived(swfDoc(path, index));

	const HINT =
		"Click to select (Alt+click: the one below) · drag or arrows move it (Shift: 10 px) · Q: scale and rotate handles, V: move · double-click or Enter edits inside a clip · Backspace goes up · nothing is written until Apply";

	/** The package list and export tree are both hidden. */
	const focused = $derived(ui.isCollapsed("side") && ui.isCollapsed("details"));
	function toggleFocus() {
		const hide = !focused;
		ui.toggle("side", hide);
		ui.toggle("details", hide);
	}

	// Only the document (the export) decides when to load: `load` reads the
	// pending edits, and tracking them would reload everything on every edit.
	$effect(() => {
		const d = doc;
		untrack(() => void d.load());
	});

	$effect(() => {
		void doc.sprite;
		void doc.version;
		const d = doc;
		untrack(() => void d.loadTimeline());
	});

	const section = $derived(
		["texts", "scripts", "images", "linkage", "tags"].includes(doc.node)
			? (doc.node as "texts" | "scripts" | "images" | "linkage" | "tags")
			: null,
	);

	/** What the stage previews. */
	const target = $derived.by((): SwfPreviewTarget | null => {
		if (doc.node === "frames") return { kind: "root", frame: doc.frame };
		const c = doc.character;
		if (!c) return null;
		if (c.kind === "sprite") {
			return doc.play
				? { kind: "character", id: c.id, play: true }
				: { kind: "frame", id: c.id, frame: doc.frame };
		}
		return { kind: "character", id: c.id };
	});

	const layout = $derived(doc.sprite !== null && !doc.play);

	/** The movie's stage in the open sprite's coordinates, when the root places it. */
	const outline = $derived.by((): SwfRect | null => {
		const stage = doc.tree?.stage;
		if (!stage) return null;
		if (doc.sprite === 0) return stage;
		const p = doc.rootPlacement;
		if (!p) return null;
		const { a, b, c, d, tx, ty } = p.matrix;
		const det = a * d - b * c;
		if (Math.abs(det) < 1e-9) return null;
		const inv = (x: number, y: number) => {
			const dx = x - tx;
			const dy = y - ty;
			return [(d * dx - c * dy) / det, (-b * dx + a * dy) / det];
		};
		const pts = [
			inv(stage.x, stage.y),
			inv(stage.x + stage.w, stage.y),
			inv(stage.x, stage.y + stage.h),
			inv(stage.x + stage.w, stage.y + stage.h),
		];
		const xs = pts.map((q) => q[0]);
		const ys = pts.map((q) => q[1]);
		const x = Math.min(...xs);
		const y = Math.min(...ys);
		return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y };
	});

	const title = $derived.by(() => {
		if (doc.node === "movie") return "Movie";
		if (doc.node === "frames") return "Root timeline";
		if (section) {
			return {
				texts: "Edit texts",
				scripts: "Scripts",
				images: "External images",
				linkage: "Linkage",
				tags: "All tags",
			}[section];
		}
		const c = doc.character;
		return c ? `${c.kind} ${c.id}${c.names[0] ? ` · ${c.names[0]}` : ""}` : "";
	});

	const zoomLabel = $derived(doc.zoom === 0 ? "fit" : `${Math.round(doc.zoom * 100)}%`);
	const zoomBy = (f: number) => {
		const z = doc.zoom || 1;
		doc.zoom = Math.max(0.05, Math.min(16, z * f));
	};
</script>

{#if doc.error && !doc.tree}
	<div class="alert alert-error text-xs">{doc.error}</div>
{:else if !doc.tree || !doc.info}
	<div class="flex items-center gap-2 p-3 text-xs opacity-60">
		<span class="loading loading-xs loading-spinner"></span> Reading the movie…
	</div>
{:else}
	<div class="flex h-full min-h-0 flex-col">
		<div class="border-base-300 flex flex-wrap items-center gap-2 border-b px-2 py-1 text-xs">
			{#if doc.trail.length > 0}
				<nav class="flex min-w-0 items-center gap-0.5" aria-label="Clip path">
					{#each doc.trail as t, k (t.id)}
						{#if k > 0}<ChevronRight size={11} class="shrink-0 opacity-40" />{/if}
						{#if k === doc.trail.length - 1}
							<span class="truncate font-semibold">{t.label}</span>
						{:else}
							<button
								class="hover:text-primary truncate opacity-70 hover:underline"
								title={t.id === 0
									? "The root timeline"
									: `Back to ${t.label} (Backspace goes up one level)`}
								onclick={() => doc.goTo(t.id)}>{t.label}</button
							>
						{/if}
					{/each}
				</nav>
			{:else}
				<span class="font-semibold">{title}</span>
			{/if}
			<span class="font-mono opacity-50"
				>{doc.info.signature} v{doc.info.version} · {doc.info.width}×{doc.info.height} · {doc
					.info.frameRate}
				fps · {doc.tree.characters.length} characters</span
			>
			<div class="flex-1"></div>
			<button
				class="btn btn-ghost btn-xs"
				title={focused
					? "Show the package and export panes again"
					: "Give the movie editor the whole window (hides the package and export panes; Ctrl+B / Ctrl+J)"}
				onclick={toggleFocus}
			>
				{#if focused}<PanelLeftOpen size={11} />{:else}<PanelLeftClose size={11} />{/if}
				Focus
			</button>
			{#if target}
				{#if doc.sprite !== null && doc.sprite !== 0}
					<div class="join">
						<button
							class="btn join-item btn-xs {doc.play ? 'btn-ghost' : 'btn-active'}"
							title="The layout the placements describe: no scripts, nested sprites on their first frame"
							onclick={() => (doc.play = false)}><Square size={11} /> Layout</button
						>
						<button
							class="btn join-item btn-xs {doc.play ? 'btn-active' : 'btn-ghost'}"
							title="Run the sprite with its timeline and scripts"
							onclick={() => (doc.play = true)}><Play size={11} /> Play</button
						>
					</div>
				{/if}
				{#if layout}
					<div class="join">
						<button
							class="btn join-item btn-xs {doc.tool === 'move'
								? 'btn-active'
								: 'btn-ghost'}"
							title="Move tool (V): drag the selection, arrows nudge"
							onclick={() => (doc.tool = "move")}><Move size={11} /></button
						>
						<button
							class="btn join-item btn-xs {doc.tool === 'transform'
								? 'btn-active'
								: 'btn-ghost'}"
							title="Free transform (Q): handles scale (Shift keeps proportions, Alt from the centre), the knob rotates (Shift: 15° steps)"
							onclick={() => (doc.tool = "transform")}><Scaling size={11} /></button
						>
					</div>
					<button
						class="btn btn-ghost btn-xs {doc.showBounds ? 'btn-active' : ''}"
						title="Show every object's bounds"
						onclick={() => (doc.showBounds = !doc.showBounds)}
						><Scan size={11} /></button
					>
				{/if}
				<div class="join">
					<button
						class="btn join-item btn-ghost btn-xs"
						title="Zoom out"
						onclick={() => zoomBy(0.8)}><ZoomOut size={11} /></button
					>
					<button
						class="btn join-item btn-ghost btn-xs w-12 font-mono"
						title="Fit (click) / 100%"
						onclick={() => (doc.zoom = doc.zoom === 0 ? 1 : 0)}>{zoomLabel}</button
					>
					<button
						class="btn join-item btn-ghost btn-xs"
						title="Zoom in (Ctrl+wheel)"
						onclick={() => zoomBy(1.25)}><ZoomIn size={11} /></button
					>
					<button
						class="btn join-item btn-ghost btn-xs"
						title="Fit"
						onclick={() => (doc.zoom = 0)}><Maximize size={11} /></button
					>
				</div>
			{/if}
			{#if doc.pending.length === 0 && studio.dirty.includes(path)}
				<button
					class="btn btn-secondary btn-xs"
					title="Open the mod cooker with this package as a delta (only the changed objects ship)"
					onclick={() => cooker.propose(path)}
					><PackagePlus size={11} /> Make a mod</button
				>
			{/if}
			{#if doc.pending.length > 0}
				<button
					class="btn btn-ghost btn-xs"
					onclick={() => doc.discardEdits()}
					title="Drop the pending edits"><RotateCcw size={11} /> Discard</button
				>
				<button
					class="btn btn-primary btn-xs"
					onclick={() => doc.apply()}
					disabled={doc.applying}
				>
					{#if doc.applying}<span class="loading loading-xs loading-spinner"
						></span>{:else}<Check size={11} />{/if}
					Apply {doc.pending.length} edit{doc.pending.length > 1 ? "s" : ""}
				</button>
			{/if}
		</div>
		<div class="flex min-h-0 flex-1">
			<Pane id="swf.tree" side="left" initial={230} min={150} max={480}>
				<SwfTree {doc} />
			</Pane>
			<div class="flex min-w-0 flex-1 flex-col">
				{#if doc.node === "movie"}
					<div class="flex h-full min-h-0 flex-col p-1">
						<p class="px-1 pb-1 text-[10px] opacity-50">
							The whole movie, emulated by Ruffle with its scripts (pending edits
							included): the package's textures are embedded and imported shared
							movies (SharedAssets, fonts, image stacks) are resolved from the
							workspace. Scaleform-only features can still differ from the game.
						</p>
						{#key doc.version}
							<SwfPlayer {path} {index} edits={doc.pending} />
						{/key}
					</div>
				{:else if section}
					<SwfSections {doc} {section} />
				{:else if target}
					{#if layout}
						<p
							class="border-base-300 truncate border-b px-2 py-0.5 text-[10px] opacity-60"
							title={HINT}
						>
							{HINT}
						</p>
					{/if}
					<SwfStage
						{doc}
						{target}
						entries={layout ? doc.entries : []}
						outline={layout ? outline : null}
					/>
					{#if layout}
						<div class="border-base-300 h-[38%] min-h-28 border-t">
							<SwfTimeline {doc} {outline} />
						</div>
					{/if}
				{/if}
			</div>
			{#if target}
				<Pane id="swf.props" side="right" initial={270} min={200} max={520}>
					<SwfProps {doc} />
				</Pane>
			{/if}
		</div>
	</div>
{/if}
