<script lang="ts">
	import { Plus } from "@lucide/svelte";
	import ValueNode from "$lib/components/ValueNode.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { ParsedObject } from "../../../electron/src/ipc/contract";

	interface Props {
		obj: Extract<ParsedObject, { kind: "object" }>;
	}
	const { obj }: Props = $props();

	let adding = $state(false);
	let newName = $state("");
	let newValue = $state("");

	/** `Name` or `Name[i]` for a static-array slot. */
	const propPath = (name: string, index: number) => (index > 0 ? `${name}[${index}]` : name);

	async function set(path: string, value: string) {
		if (!studio.pkg) return;
		try {
			const updated = await rpc("object.set", {
				path: studio.pkg.path,
				index: obj.index,
				prop: path,
				value,
			});
			studio.object = updated;
			studio.markDirty();
		} catch (error) {
			toasts.error(error, `Could not set ${path}`);
		}
	}

	async function list(
		path: string,
		op: "add" | "duplicate" | "remove" | "removeProperty",
		at?: number,
	) {
		if (!studio.pkg) return;
		try {
			const updated = await rpc("object.list", {
				path: studio.pkg.path,
				index: obj.index,
				prop: path,
				op,
				at,
			});
			studio.object = updated;
			studio.markDirty();
		} catch (error) {
			toasts.error(error, `Could not change ${path}`);
		}
	}

	async function addProperty() {
		const name = newName.trim();
		if (!name) return;
		await set(name, newValue);
		adding = false;
		newName = "";
		newValue = "";
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
				/>
			{/each}
		</tbody>
	</table>
	{#if obj.knownClass && !obj.isClass}
		<div class="mt-2 px-1">
			{#if adding}
				<form
					class="flex items-center gap-1"
					onsubmit={(e) => {
						e.preventDefault();
						void addProperty();
					}}
				>
					<input
						class="input input-xs w-48 font-mono"
						placeholder="PropertyName"
						bind:value={newName}
					/>
					<span class="opacity-50">=</span>
					<input
						class="input input-xs flex-1 font-mono"
						placeholder="value"
						bind:value={newValue}
					/>
					<button class="btn btn-xs btn-primary" type="submit">Set</button>
					<button
						class="btn btn-ghost btn-xs"
						type="button"
						onclick={() => (adding = false)}>Cancel</button
					>
				</form>
				<p class="mt-1 text-[11px] opacity-60">
					The property must be declared on the class (or a parent); it is created with the
					tag type reflection gives it. Paths like <code>Name[2].Member</code> work too.
				</p>
			{:else}
				<button class="btn btn-ghost btn-xs" onclick={() => (adding = true)}>
					<Plus size={12} /> Set a property not listed
				</button>
			{/if}
		</div>
	{/if}
</div>
