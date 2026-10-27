<script lang="ts">
	import { ChevronDown, ChevronRight, CopyPlus, Plus, RotateCcw, X } from "@lucide/svelte";
	import ScalarEditor from "$lib/components/ScalarEditor.svelte";
	import ValueNode from "$lib/components/ValueNode.svelte";
	import type { PropValue } from "../../../electron/src/ipc/contract";

	interface Props {
		label: string;
		/** Property path for `object.set` (`Outer.Inner[2].Leaf`). */
		path: string;
		value: PropValue;
		type: string;
		structName?: string;
		enumName?: string;
		depth: number;
		/** Element type for arrays, as a hint only (unknown to the server's JSON). */
		onset: (path: string, value: string) => Promise<void>;
		/** Binary struct members with one element are edited at `path` directly. */
		editable?: boolean;
		/** Structural edits: add/duplicate/remove an array element, reset a property. */
		onlist?: (
			path: string,
			op: "add" | "duplicate" | "remove" | "removeProperty",
			at?: number,
		) => Promise<void>;
		/** Set on array elements: the array's path and this element's index. */
		element?: { array: string; at: number };
	}
	const {
		label,
		path,
		value,
		type,
		structName,
		enumName,
		depth,
		onset,
		editable = true,
		onlist,
		element,
	}: Props = $props();

	const compound = $derived(value.t === "struct" || value.t === "binary" || value.t === "array");
	const count = $derived(
		value.t === "array"
			? (value.len ?? value.v.length)
			: value.t === "struct" || value.t === "binary"
				? value.v.length
				: 0,
	);
	// Initial expansion only: the user owns the state afterwards.
	// svelte-ignore state_referenced_locally
	let open = $state(depth < 1 && !(value.t === "array" && (value.len ?? value.v.length) > 64));

	const typeLabel = $derived.by(() => {
		if (value.t === "struct" || value.t === "binary") return structName ?? "struct";
		if (value.t === "array") return `array[${count}]`;
		if (value.t === "enum") return enumName ?? "enum";
		return type.replace(/property$/i, "").toLowerCase();
	});
</script>

<tr class="group">
	<td class="w-[45%] py-0.5 align-top whitespace-nowrap" style="padding-left: {depth * 14 + 4}px">
		{#if compound}
			<button
				class="hover:text-primary inline-flex items-center gap-1"
				onclick={() => (open = !open)}
			>
				{#if open}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}
				<span>{label}</span>
			</button>
		{:else}
			<span class="ml-4">{label}</span>
		{/if}
		<span class="ml-2 text-[10px] opacity-40">{typeLabel}</span>
		{#if onlist && editable}
			<span
				class="ml-1 inline-flex gap-0.5 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
			>
				{#if value.t === "array"}
					<button
						class="btn btn-ghost btn-xs btn-square h-4 min-h-4 w-4"
						title="Add an element (a copy of the last one)"
						aria-label="Add an element"
						onclick={() => {
							open = true;
							void onlist(path, "add");
						}}><Plus size={10} /></button
					>
				{/if}
				{#if element}
					<button
						class="btn btn-ghost btn-xs btn-square h-4 min-h-4 w-4"
						title="Duplicate this element"
						aria-label="Duplicate this element"
						onclick={() => onlist(element.array, "duplicate", element.at)}
						><CopyPlus size={10} /></button
					>
					<button
						class="btn btn-ghost btn-xs btn-square hover:text-error h-4 min-h-4 w-4"
						title="Remove this element"
						aria-label="Remove this element"
						onclick={() => onlist(element.array, "remove", element.at)}
						><X size={10} /></button
					>
				{:else if depth === 0}
					<button
						class="btn btn-ghost btn-xs btn-square h-4 min-h-4 w-4"
						title="Reset to the class default (remove the property from this object)"
						aria-label="Reset to default"
						onclick={() => onlist(path, "removeProperty")}
						><RotateCcw size={10} /></button
					>
				{/if}
			</span>
		{/if}
	</td>
	<td class="py-0.5 align-top">
		{#if value.t === "struct"}
			<span class="opacity-40">{"{"} {count} {"}"}</span>
		{:else if value.t === "binary"}
			<span class="opacity-40">{"{"} {count} members, binary {"}"}</span>
		{:else if value.t === "array"}
			<span class="opacity-40">[ {count} ]</span>
		{:else}
			<ScalarEditor {value} {type} {path} {editable} {onset} />
		{/if}
	</td>
</tr>
{#if compound && open}
	{#if value.t === "struct"}
		{#each value.v as p, k (k)}
			<ValueNode
				label={p.index > 0 ? `${p.name}[${p.index}]` : p.name}
				path={`${path}.${p.index > 0 ? `${p.name}[${p.index}]` : p.name}`}
				value={p.value}
				type={p.type}
				structName={p.struct}
				enumName={p.enum}
				depth={depth + 1}
				{onset}
				{onlist}
			/>
		{/each}
	{:else if value.t === "binary"}
		{#each value.v as m, k (k)}
			{#if m.elems.length === 1}
				<ValueNode
					label={m.name}
					path={`${path}.${m.name}`}
					value={m.elems[0]!}
					type={m.type}
					depth={depth + 1}
					{onset}
				/>
			{:else}
				{#each m.elems as e, j (j)}
					<ValueNode
						label={`${m.name}[${j}]`}
						path={`${path}.${m.name}[${j}]`}
						value={e}
						type={m.type}
						depth={depth + 1}
						{onset}
					/>
				{/each}
			{/if}
		{/each}
	{:else if value.t === "array"}
		{#each value.v.slice(0, 2000) as e, j (j)}
			<ValueNode
				label={`[${j}]`}
				path={`${path}[${j}]`}
				value={e}
				{type}
				depth={depth + 1}
				{onset}
				{onlist}
				element={{ array: path, at: j }}
			/>
		{/each}
		{#if count > 2000}
			<tr
				><td
					colspan="2"
					class="text-xs opacity-60"
					style="padding-left: {(depth + 1) * 14 + 4}px"
					>… {count - 2000} more elements</td
				></tr
			>
		{/if}
	{/if}
{/if}
