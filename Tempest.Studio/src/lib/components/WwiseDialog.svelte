<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import WwiseMediaList from "$lib/components/WwiseMediaList.svelte";
	import { bytes } from "$lib/studio/format";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { WwisePackage } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let packages = $state<{ name: string; path: string; size: number }[]>([]);
	let filter = $state("");
	let selected = $state<string | null>(null);
	let pkg = $state<WwisePackage | null>(null);
	let loading = $state(false);
	let eventFilter = $state("");
	let highlight = $state<Set<number> | undefined>(undefined);
	let selectedEvent = $state<number | null>(null);

	$effect(() => {
		if (!open) return;
		void (async () => {
			try {
				packages = (await rpc("wwise.packages", {})).packages;
			} catch (error) {
				toasts.error(error, "Wwise");
			}
		})();
	});

	const shown = $derived(
		packages.filter((p) => p.name.toLowerCase().includes(filter.toLowerCase())),
	);

	async function openPackage(path: string) {
		selected = path;
		loading = true;
		pkg = null;
		highlight = undefined;
		selectedEvent = null;
		try {
			pkg = await rpc("wwise.open", { path });
		} catch (error) {
			toasts.error(error, "Cannot open package");
		} finally {
			loading = false;
		}
	}

	/** Read the open package again (a replace, save or discard), keeping the event picked. */
	async function reload() {
		if (!selected) return;
		try {
			pkg = await rpc("wwise.open", { path: selected });
		} catch (error) {
			toasts.error(error, "Cannot open package");
		}
	}

	// Saving or discarding the .pck changes what it holds.
	let seenEdits = studio.edits;
	$effect(() => {
		const e = studio.edits;
		if (e !== seenEdits) {
			seenEdits = e;
			void reload();
		}
	});

	const allMedia = $derived(pkg ? [...pkg.banks.flatMap((b) => b.media), ...pkg.streams] : []);
	const events = $derived(
		pkg
			? pkg.banks
					.flatMap((b) => b.events)
					.filter((e) =>
						(e.name ?? String(e.id)).toLowerCase().includes(eventFilter.toLowerCase()),
					)
			: [],
	);

	function pickEvent(e: { id: number; media: number[] }) {
		if (selectedEvent === e.id) {
			selectedEvent = null;
			highlight = undefined;
		} else {
			selectedEvent = e.id;
			highlight = new Set(e.media);
		}
	}

	const listed = $derived(highlight ? allMedia.filter((m) => highlight?.has(m.id)) : allMedia);
</script>

<Modal bind:open title="Wwise audio" width="max-w-6xl">
	<div class="flex h-[70vh] gap-3 text-sm">
		<div class="flex w-64 shrink-0 flex-col gap-1">
			<input
				class="input input-sm font-mono text-xs"
				placeholder="filter packages…"
				bind:value={filter}
			/>
			<div class="border-base-300 flex-1 overflow-auto rounded border">
				{#each shown as p (p.path)}
					<button
						class="hover:bg-base-200 flex w-full items-center gap-2 px-2 py-0.5 text-left font-mono text-xs {selected ===
						p.path
							? 'bg-primary/15'
							: ''}"
						onclick={() => openPackage(p.path)}
					>
						<span class="flex-1 truncate">{p.name}</span>
						<span class="opacity-50">{bytes(p.size)}</span>
					</button>
				{/each}
			</div>
			<p class="text-[11px] opacity-60">{packages.length} packages</p>
		</div>
		<div class="flex min-w-0 flex-1 flex-col gap-2">
			{#if loading}
				<span class="loading loading-sm loading-spinner"></span>
			{:else if pkg}
				<div class="text-xs opacity-70">
					<b>{pkg.name}</b> · {pkg.banks.length} bank{pkg.banks.length === 1 ? "" : "s"} ·
					{pkg.banks.reduce((n, b) => n + b.media.length, 0)} embedded · {pkg.streams
						.length} streamed · languages {pkg.languages.join(", ") || "-"}
				</div>
				<div class="flex min-h-0 flex-1 gap-2">
					<div class="flex w-72 shrink-0 flex-col gap-1">
						<input
							class="input input-xs font-mono"
							placeholder="filter events…"
							bind:value={eventFilter}
						/>
						<div class="border-base-300 flex-1 overflow-auto rounded border">
							{#each events as e (e.id)}
								<button
									class="hover:bg-base-200 block w-full truncate px-2 py-0.5 text-left font-mono text-[11px] {selectedEvent ===
									e.id
										? 'bg-primary/15'
										: ''}"
									onclick={() => pickEvent(e)}
									title={`${e.media.length} media`}
								>
									{e.name ?? e.id}
									<span class="opacity-40">({e.media.length})</span>
								</button>
							{/each}
							{#if events.length === 0}
								<p class="p-2 text-[11px] opacity-50">No events.</p>
							{/if}
						</div>
					</div>
					<div class="min-w-0 flex-1 overflow-auto">
						{#key `${pkg.path}#${selectedEvent}`}
							<WwiseMediaList
								pck={pkg.path}
								media={listed}
								{highlight}
								onchanged={() => void reload()}
							/>
						{/key}
					</div>
				</div>
			{:else}
				<p class="opacity-60">
					Pick a package. Events on the left filter the media to what they play.
				</p>
			{/if}
		</div>
	</div>
</Modal>
