<!--
	A sprite's (or the root's) timeline under the stage: a frame strip with
	labels and frame scripts when there is more than one frame, then the
	display list of the current frame. A row selects the object (the stage
	highlights it), double-click opens its character. Objects outside the
	stage and objects with pending edits are marked.
-->
<script lang="ts">
	import { EyeOff, FileCode, Pencil } from "@lucide/svelte";
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

	function hidden(e: SwfEntry): boolean {
		return (e.cxform?.mul[3] ?? 1) === 0;
	}

	const fmt = (v: number, d = 1) =>
		Math.abs(v) < 1e-9 ? "0" : v.toFixed(d).replace(/\.0+$/, "");

	function onKeyDown(event: KeyboardEvent) {
		if (inField(event)) return;
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
		} else if (event.key === "Enter" && doc.selectedEntry) {
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
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="min-h-0 flex-1 overflow-auto" onkeydown={onKeyDown}>
		<table class="table-xs table-pin-rows table font-mono">
			<thead>
				<tr>
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
							: 'hover:bg-base-300'}"
						data-swf-depth={e.depth}
						tabindex="-1"
						onclick={() => (doc.depth = e.depth)}
						ondblclick={() => doc.openCharacter(e.character)}
					>
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
					<tr><td colspan="9" class="opacity-60">Nothing on this frame.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>
