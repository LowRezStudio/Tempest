<!--
	A sprite's (or the root's) timeline under the stage: a frame strip with
	labels and frame scripts when there is more than one frame, then the
	display list of the current frame. A row selects the object (the stage
	highlights it), double-click opens its character. Objects outside the
	stage and objects with pending edits are marked. The eye column hides a
	layer from the stage render (editor only, the movie is not changed), with
	show all / hide all / hide off-stage above the list.
-->
<script lang="ts">
	import { Eye, EyeOff, FileCode, Pencil } from "@lucide/svelte";
	import { inField, reveal, step } from "$lib/studio/keys";
	import { decompose, type SwfDoc } from "$lib/studio/swf.svelte";
	import type { SwfEntry, SwfRect } from "../../../../electron/src/ipc/contract";

	interface Props {
		doc: SwfDoc;
		/** The stage in the timeline's coordinates, to flag objects outside it. */
		outline?: SwfRect | null;
	}
	const { doc, outline = null }: Props = $props();

	const t = $derived(doc.timeline);
	const frames = $derived(t?.frames.length ?? 0);
	const labelAt = $derived(new Map((t?.labels ?? []).map((l) => [l.frame, l.name])));
	const scriptAt = $derived(new Set((t?.actions ?? []).map((a) => a.frame)));

	function offStage(e: SwfEntry): boolean {
		if (!outline || !e.rect) return false;
		const r = e.rect;
		return (
			r.x >= outline.x + outline.w ||
			r.y >= outline.y + outline.h ||
			r.x + r.w <= outline.x ||
			r.y + r.h <= outline.y
		);
	}

	const offStageDepths = $derived(
		doc.entries.filter((e) => offStage(e) && !doc.hidden.includes(e.depth)).map((e) => e.depth),
	);
	/**
	 * Visible layers drawn over nearly the same rectangle as each layer:
	 * stacked states (GoToScene/Event/Chest/BattlePass share one spot), so
	 * hiding one changes nothing visible until the others are hidden too.
	 */
	const stacked = $derived.by(() => {
		const out = new Map<number, string[]>();
		// Overlap over union: the stacked states differ by a few pixels in size.
		const near = (a: SwfRect, b: SwfRect) => {
			const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
			const h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
			if (w <= 0 || h <= 0) return false;
			const inter = w * h;
			return inter / (a.w * a.h + b.w * b.h - inter) >= 0.75;
		};
		const shown = doc.entries.filter((e) => e.rect && !doc.hidden.includes(e.depth));
		for (const e of doc.entries) {
			if (!e.rect) continue;
			const others = shown.filter((o) => o.depth !== e.depth && near(o.rect!, e.rect!));
			if (others.length > 0) {
				out.set(
					e.depth,
					others.map((o) => o.name ?? doc.label(o.character)),
				);
			}
		}
		return out;
	});

	/** Show only these layers. */
	function solo(depths: number[]) {
		doc.setHidden(
			doc.entries.map((e) => e.depth).filter((d) => !depths.includes(d)),
			true,
		);
		doc.setHidden(depths, false);
	}

	const hiddenHere = $derived(doc.entries.filter((e) => doc.hidden.includes(e.depth)).length);

	function hidden(e: SwfEntry): boolean {
		return (e.cxform?.mul[3] ?? 1) === 0;
	}

	const fmt = (v: number, d = 1) =>
		Math.abs(v) < 1e-9 ? "0" : v.toFixed(d).replace(/\.0+$/, "");

	function onKeyDown(event: KeyboardEvent) {
		// Alt+← / Alt+→ are Back and Forward.
		if (inField(event) || event.altKey) return;
		const list = doc.entries;
		if (event.key === "ArrowDown" || event.key === "ArrowUp") {
			event.preventDefault();
			const i = step(
				list.findIndex((e) => e.depth === doc.depth),
				event.key === "ArrowDown" ? 1 : -1,
				list.length,
			);
			if (i >= 0) {
				doc.depth = list[i].depth;
				reveal(`[data-swf-depth="${list[i].depth}"]`);
			}
		} else if ((event.key === "ArrowLeft" || event.key === "ArrowRight") && frames > 1) {
			event.preventDefault();
			doc.frame = Math.max(
				0,
				Math.min(frames - 1, doc.frame + (event.key === "ArrowRight" ? 1 : -1)),
			);
		} else if (
			(event.key === "Enter" ||
				(event.key === " " && !(event.target as HTMLElement).closest("button"))) &&
			doc.selectedEntry
		) {
			event.preventDefault();
			doc.openCharacter(doc.selectedEntry.character);
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col text-xs">
	{#if frames > 1}
		<div class="border-base-300 flex items-center gap-2 border-b px-2 py-1">
			<span class="font-mono opacity-60">frame {doc.frame + 1}/{frames}</span>
			<div class="flex min-w-0 flex-1 overflow-x-auto">
				{#each { length: frames } as _, f (f)}
					<button
						class="border-base-100 relative h-5 min-w-3 flex-1 border-r {f === doc.frame
							? 'bg-primary'
							: labelAt.has(f)
								? 'bg-secondary/40 hover:bg-secondary/60'
								: 'bg-base-300 hover:bg-base-content/20'}"
						title="frame {f + 1}{labelAt.has(f)
							? ` · ${labelAt.get(f)}`
							: ''}{scriptAt.has(f) ? ' · script' : ''}"
						onclick={() => (doc.frame = f)}
					>
						{#if scriptAt.has(f)}<span
								class="bg-accent absolute top-0.5 left-0.5 size-1 rounded-full"
							></span>{/if}
					</button>
				{/each}
			</div>
			{#if labelAt.has(doc.frame)}<span class="badge badge-secondary badge-xs"
					>{labelAt.get(doc.frame)}</span
				>{/if}
		</div>
	{/if}
	<div class="border-base-300 flex items-center gap-1 border-b px-2 py-0.5">
		<span class="opacity-60"
			>{doc.entries.length} layer{doc.entries.length === 1 ? "" : "s"}{doc.hidden.length > 0
				? ` · ${hiddenHere} hidden in the view`
				: ""}</span
		>
		<div class="flex-1"></div>
		<button
			class="btn btn-ghost btn-xs"
			title="Hide every layer that lies entirely outside the stage"
			disabled={offStageDepths.length === 0}
			onclick={() => doc.setHidden(offStageDepths, true)}
			><EyeOff size={11} /> Hide off-stage{offStageDepths.length > 0
				? ` (${offStageDepths.length})`
				: ""}</button
		>
		<button
			class="btn btn-ghost btn-xs"
			title="Show only the selected layer (Alt+click an eye does the same for its layer)"
			disabled={doc.depth === null}
			onclick={() => doc.depth !== null && solo([doc.depth])}
			><Eye size={11} /> Solo selected</button
		>
		<button
			class="btn btn-ghost btn-xs"
			title="Hide every layer (then tick the ones to look at)"
			onclick={() =>
				doc.setHidden(
					doc.entries.map((e) => e.depth),
					true,
				)}><EyeOff size={11} /> Hide all</button
		>
		<button
			class="btn btn-ghost btn-xs"
			title="Show every layer again"
			disabled={doc.hidden.length === 0}
			onclick={() => doc.setHidden(doc.hidden, false)}><Eye size={11} /> Show all</button
		>
	</div>
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="min-h-0 flex-1 overflow-auto" onkeydown={onKeyDown}>
		<table class="table-xs table-pin-rows table font-mono">
			<thead>
				<tr>
					<th class="w-6" title="Shown in the stage render (editor only)"
						><Eye size={11} /></th
					>
					<th>depth</th>
					<th>name</th>
					<th>character</th>
					<th class="text-right">x</th>
					<th class="text-right">y</th>
					<th class="text-right">scale</th>
					<th class="text-right">rot</th>
					<th class="text-right">alpha</th>
					<th>tag</th>
				</tr>
			</thead>
			<tbody>
				{#each doc.entries as e (e.depth)}
					{@const d = decompose(e.matrix)}
					{@const c = doc.chars.get(e.character)}
					<tr
						class="cursor-pointer {e.depth === doc.depth
							? 'bg-primary/20'
							: 'hover:bg-base-300'} {doc.hidden.includes(e.depth)
							? 'opacity-45'
							: ''}"
						data-swf-depth={e.depth}
						tabindex="-1"
						onclick={() => (doc.depth = e.depth)}
						ondblclick={() => doc.openCharacter(e.character)}
					>
						<td>
							<button
								class="btn btn-ghost btn-xs btn-square h-5 min-h-5 w-5"
								aria-pressed={!doc.hidden.includes(e.depth)}
								aria-label={doc.hidden.includes(e.depth)
									? "Show this layer"
									: "Hide this layer"}
								title={`${doc.hidden.includes(e.depth) ? "Hidden in the stage render: click to show it" : "Click to hide it in the stage render (the movie is not changed)"}. Alt+click: show only this layer.`}
								data-swf-eye
								onclick={(ev) => {
									ev.stopPropagation();
									if (ev.altKey) solo([e.depth]);
									else doc.setHidden([e.depth], !doc.hidden.includes(e.depth));
								}}
								ondblclick={(ev) => ev.stopPropagation()}
							>
								{#if doc.hidden.includes(e.depth)}<EyeOff
										size={12}
										class="opacity-50"
									/>{:else}<Eye size={12} />{/if}
							</button>
						</td>
						<td class="opacity-60">
							{e.depth}{#if e.clipDepth !== null}<span
									title="mask up to depth {e.clipDepth}">⌁{e.clipDepth}</span
								>{/if}
						</td>
						<td>
							<span class="inline-flex items-center gap-1">
								{e.name ?? ""}
								{#if doc.entryEdited(e)}<span title="pending edit"
										><Pencil size={10} class="text-warning" /></span
									>{/if}
								{#if stacked.has(e.depth) && !doc.hidden.includes(e.depth)}<span
										class="badge badge-info badge-xs"
										title="Drawn at the same place as {stacked
											.get(e.depth)!
											.join(
												', ',
											)}: hide those too to see what is under (or Alt+click the eye to show only this one)"
										>stacked ×{stacked.get(e.depth)!.length + 1}</span
									>{/if}
								{#if offStage(e)}<span
										class="badge badge-warning badge-xs"
										title="entirely outside the stage">off stage</span
									>{/if}
								{#if hidden(e)}<span title="alpha 0"
										><EyeOff size={10} class="opacity-60" /></span
									>{/if}
							</span>
						</td>
						<td class="max-w-48 truncate" title="{c?.kind ?? '?'} {e.character}">
							<span class="opacity-50">{c?.kind ?? "?"}</span>
							{doc.label(e.character)}
							{#if c?.attached.some((a) => a.name === "DoInitAction")}<span
									title="has class code"
									><FileCode size={10} class="inline opacity-50" /></span
								>{/if}
						</td>
						<td class="text-right">{fmt(d.x, 2)}</td>
						<td class="text-right">{fmt(d.y, 2)}</td>
						<td class="text-right"
							>{d.sx === 1 && d.sy === 1
								? ""
								: `${fmt(d.sx * 100)}%${d.sx === d.sy ? "" : `×${fmt(d.sy * 100)}%`}`}</td
						>
						<td class="text-right">{d.rot ? `${fmt(d.rot)}°` : ""}</td>
						<td class="text-right"
							>{e.cxform ? `${fmt(e.cxform.mul[3] * 100)}%` : ""}</td
						>
						<td
							class="opacity-50"
							title="placed by tag {e.placedBy}, matrix from tag {e.matrixBy}"
							>#{e.placedBy}{e.matrixBy !== e.placedBy ? `→${e.matrixBy}` : ""}</td
						>
					</tr>
				{:else}
					<tr><td colspan="10" class="opacity-60">Nothing on this frame.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>
