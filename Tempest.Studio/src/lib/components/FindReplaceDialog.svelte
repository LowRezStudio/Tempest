<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { ReplaceMatch } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let query = $state("");
	let replacement = $state("");
	let names = $state(true);
	let matches = $state<ReplaceMatch[] | null>(null);
	let working = $state(false);
	let searchedFor = $state("");

	$effect(() => {
		if (open) matches = null;
	});

	async function find(event?: Event) {
		event?.preventDefault();
		if (!studio.pkg || query.length < 1) return;
		working = true;
		try {
			const r = await rpc("package.findReplace", {
				path: studio.pkg.path,
				query,
				replacement,
				names,
				apply: false,
			});
			matches = r.matches;
			searchedFor = query;
		} catch (error) {
			toasts.error(error, "Find failed");
		} finally {
			working = false;
		}
	}

	async function replaceAll() {
		if (!studio.pkg || !matches?.length) return;
		working = true;
		const path = studio.pkg.path;
		const selected = studio.selected;
		try {
			const r = await studio.run("Replacing…", () =>
				rpc("package.findReplace", { path, query, replacement, names, apply: true }),
			);
			for (const e of r.errors.slice(0, 5)) {
				toasts.push(
					"warning",
					`${e.object} ${e.prop ?? "(name)"}: ${e.error}`,
					"Not replaced",
				);
			}
			toasts.push(
				"success",
				`${r.applied} replaced${r.errors.length ? `, ${r.errors.length} failed` : ""}. Save the package to write it.`,
				"Replace all",
			);
			studio.markDirty(path);
			await studio.reloadTables();
			if (selected !== null) await studio.select(selected);
			open = false;
		} catch (error) {
			toasts.error(error, "Replace failed");
		} finally {
			working = false;
		}
	}

	const kindLabel = (k: ReplaceMatch["kind"]) =>
		k === "objectname" ? "object name" : k === "str" ? "string" : k;
</script>

<Modal bind:open title="Find and replace in {studio.pkg?.name ?? ''}" width="max-w-4xl">
	<form class="space-y-2" onsubmit={find}>
		<div class="flex gap-2">
			<!-- svelte-ignore a11y_autofocus -->
			<input
				class="input input-sm flex-1 font-mono"
				placeholder="find (case-insensitive substring)"
				bind:value={query}
				autofocus
			/>
			<input
				class="input input-sm flex-1 font-mono"
				placeholder="replace with"
				bind:value={replacement}
			/>
			<button class="btn btn-sm btn-primary" type="submit" disabled={working || !query}>
				{#if working}<span class="loading loading-xs loading-spinner"></span>{/if}
				Find
			</button>
		</div>
		<label class="flex items-center gap-2 text-xs">
			<input type="checkbox" class="checkbox checkbox-xs" bind:checked={names} />
			Also object names (renames the exports; references keep pointing at them)
		</label>
		<p class="text-[11px] opacity-60">
			Searches every string, name, enum and object-reference value of every export of the open
			package. Strings and names get the substring replaced; an object reference is
			re-resolved from the replaced path, so it must name an object of this package.
		</p>
	</form>

	{#if matches}
		<div class="mt-3 flex items-center gap-2 text-sm">
			<span>
				<b>{matches.length}</b> match{matches.length === 1 ? "" : "es"} for
				<code>{searchedFor}</code>
			</span>
			<div class="flex-1"></div>
			<button
				class="btn btn-sm btn-primary"
				onclick={replaceAll}
				disabled={working || matches.length === 0}
			>
				Replace all
			</button>
		</div>
		<div class="mt-2 max-h-[50vh] overflow-auto">
			<table class="table-xs table w-full table-fixed font-mono">
				<thead>
					<tr
						><th class="w-[30%]">object</th><th class="w-[22%]">property</th><th
							class="w-[8%]">kind</th
						><th class="w-[20%]">current</th><th class="w-[20%]">new</th></tr
					>
				</thead>
				<tbody>
					{#each matches.slice(0, 2000) as m, k (k)}
						<tr>
							<td class="truncate">
								<button
									class="link link-primary"
									onclick={() => studio.select(m.index)}>{m.object}</button
								>
							</td>
							<td class="truncate opacity-80" title={m.prop ?? "(name)"}
								>{m.prop ?? "(name)"}</td
							>
							<td class="opacity-60">{kindLabel(m.kind)}</td>
							<td class="selectable truncate" title={m.current}>{m.current}</td>
							<td
								class="selectable text-success max-w-xs truncate"
								title={m.replacement}>{m.replacement}</td
							>
						</tr>
					{/each}
				</tbody>
			</table>
			{#if matches.length > 2000}
				<p class="mt-1 text-xs opacity-60">first 2000 shown; all are replaced</p>
			{/if}
		</div>
	{/if}
</Modal>
