<script lang="ts">
	import { Plus } from "@lucide/svelte";
	import AddPropertyPicker from "$lib/components/AddPropertyPicker.svelte";
	import ValueNode from "$lib/components/ValueNode.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { ParsedObject } from "../../../electron/src/ipc/contract";

	interface Props {
		obj: Extract<ParsedObject, { kind: "object" }>;
		/** The package the object is in (default: the open package). */
		path?: string;
		/** Called with the object after an edit (default: shown as the open package's object). */
		onchange?: (updated: ParsedObject) => void;
	}
	const { obj, path: pkgPath, onchange }: Props = $props();

	const target = $derived(pkgPath ?? studio.pkg?.path ?? null);

	function changed(updated: ParsedObject) {
		if (onchange) onchange(updated);
		else studio.object = updated;
		studio.markDirty(target ?? undefined);
	}

	let adding = $state(false);

	/** `Name` or `Name[i]` for a static-array slot. */
	const propPath = (name: string, index: number) => (index > 0 ? `${name}[${index}]` : name);

	async function set(path: string, value: string) {
		if (!target) return;
		try {
			const updated = await rpc("object.set", {
				path: target,
				index: obj.index,
				prop: path,
				value,
			});
			changed(updated);
		} catch (error) {
			toasts.error(error, `Could not set ${path}`);
		}
	}

	async function list(
		path: string,
		op: "add" | "duplicate" | "remove" | "removeProperty",
		at?: number,
	) {
		if (!target) return;
		try {
			const updated = await rpc("object.list", {
				path: target,
				index: obj.index,
				prop: path,
				op,
				at,
			});
			changed(updated);
		} catch (error) {
			toasts.error(error, `Could not change ${path}`);
		}
	}
</script>

<div class="p-2">
	{#if obj.properties.length === 0}
		<p class="px-1 py-2 text-xs opacity-60">
			No tagged properties (every value is the class default).
		</p>
	{/if}
	<table class="table-xs table font-mono">
		<tbody>
			{#each obj.properties as p, k (k)}
				<ValueNode
					label={propPath(p.name, p.index)}
					path={propPath(p.name, p.index)}
					value={p.value}
					type={p.type}
					structName={p.struct}
					enumName={p.enum}
					depth={0}
					onset={set}
					onlist={list}
					index={obj.index}
					pkgPath={target ?? undefined}
					onadded={onchange}
				/>
			{/each}
		</tbody>
	</table>
	{#if obj.knownClass && !obj.isClass}
		<div class="mt-2 px-1">
			{#if adding}
				<AddPropertyPicker
					index={obj.index}
					path={target ?? undefined}
					onadded={onchange}
					onclose={() => (adding = false)}
				/>
			{:else}
				<button class="btn btn-ghost btn-xs" onclick={() => (adding = true)}>
					<Plus size={12} /> Add a property…
				</button>
			{/if}
		</div>
	{/if}
</div>
