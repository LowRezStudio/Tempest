<!--
	The properties an object (or one of its structs, `at`) can have but does
	not store, from the class declarations (`object.missing`): pick one to add
	it at its class default (`object.addProperty`).
-->
<script lang="ts">
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";

	interface Props {
		index: number;
		/** The struct the property goes into; empty for the object itself. */
		at?: string;
		onclose: () => void;
	}
	const { index, at = "", onclose }: Props = $props();

	let list = $state<{ name: string; kind: string; type: string }[] | null>(null);
	let filter = $state("");
	let error = $state<string | null>(null);

	$effect(() => {
		const pkg = studio.pkg;
		if (!pkg) return;
		void rpc("object.missing", { path: pkg.path, index, at: at || undefined })
			.then((r) => (list = r))
			.catch(
				(error_: unknown) =>
					(error = error_ instanceof Error ? error_.message : String(error_)),
			);
	});

	const shown = $derived(
		(list ?? []).filter((p) => p.name.toLowerCase().includes(filter.trim().toLowerCase())),
	);

	async function add(name: string) {
		const pkg = studio.pkg;
		if (!pkg) return;
		try {
			studio.object = await rpc("object.addProperty", {
				path: pkg.path,
				index,
				name,
				at: at || undefined,
			});
			studio.markDirty();
			onclose();
		} catch (error_) {
			toasts.error(error_, `Could not add ${name}`);
		}
	}
</script>

<div class="bg-base-200 my-1 flex flex-col gap-1 rounded p-2 font-sans text-xs">
	<div class="flex items-center gap-1">
		<input
			class="input input-xs flex-1 font-mono"
			placeholder={at ? `a field of ${at}` : "a property of this object"}
			bind:value={filter}
		/>
		<button class="btn btn-ghost btn-xs" onclick={onclose}>Cancel</button>
	</div>
	{#if error}
		<p class="text-error">{error}</p>
	{:else if !list}
		<span class="loading loading-xs loading-spinner"></span>
	{:else if list.length === 0}
		<p class="opacity-60">Every property it can have is already set.</p>
	{:else}
		<div class="max-h-48 overflow-y-auto">
			{#each shown as p (p.name)}
				<button
					class="hover:bg-base-300 flex w-full items-baseline gap-2 rounded px-1 py-0.5 text-left"
					onclick={() => add(p.name)}
				>
					<span class="font-mono">{p.name}</span>
					<span class="opacity-50">{p.type || p.kind}</span>
				</button>
			{/each}
		</div>
		<p class="opacity-60">It starts at its default, so nothing changes until you edit it.</p>
	{/if}
</div>
