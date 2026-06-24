<script lang="ts">
	import { Box, RefreshCw, Trash2 } from "@lucide/svelte";
	import ModMenu from "$lib/components/mods/ModMenu.svelte";
	import { m } from "$lib/paraglide/messages";
	import type { ModRecord } from "$lib/core/mods";

	interface Props {
		mods: ModRecord[];
		gamePath: string;
		isLoading: boolean;
		isRemoving?: boolean;
		onRefresh: () => void;
		onRemoveMod: (name: string) => void;
		onOpenDetails: (mod: ModRecord) => void;
	}

	let {
		mods,
		gamePath,
		isLoading,
		isRemoving = false,
		onRefresh,
		onRemoveMod,
		onOpenDetails,
	}: Props = $props();
</script>

<table class="table">
	<thead>
		<tr>
			<th>
				<button class="flex items-center gap-1 font-semibold text-sm">
					<span>{m.common_name()}</span>
				</button>
			</th>
			<th class="w-48">{m.common_version()}</th>
			<th class="w-auto text-right">
				<button class="btn btn-ghost btn-sm" onclick={onRefresh} disabled={isLoading}>
					{#if isLoading}
						<span class="loading loading-spinner loading-xs"></span>
					{:else}
						<RefreshCw size={14} />
					{/if}
					{m.common_refresh()}
				</button>
			</th>
		</tr>
	</thead>
	<tbody>
		{#each mods as mod (mod.Id)}
			<tr class="hover">
				<td>
					<div class="flex items-center gap-3">
						<button
							class="w-10 h-10 rounded-lg bg-base-200 hover:bg-base-300 flex items-center justify-center shrink-0 transition-all text-primary hover:scale-105 active:scale-95 cursor-pointer"
							onclick={() => onOpenDetails(mod)}
							title={m.mod_updated_files()}
						>
							<Box size={20} class="opacity-75" />
						</button>
						<div class="flex-1 min-w-0">
							<div class="flex items-center gap-2">
								<button
									class="font-bold text-sm truncate text-left hover:text-primary transition-colors cursor-pointer"
									onclick={() => onOpenDetails(mod)}
									title={m.mod_updated_files()}
								>
									{mod.Name}
								</button>
							</div>
							{#if mod.Authors && mod.Authors.length > 0 && mod.Authors.some((a) => a.Link)}
								<button
									class="text-xs opacity-70 hover:opacity-100 cursor-pointer text-left block"
									onclick={() => onOpenDetails(mod)}
								>
									by <span
										class="underline decoration-dashed decoration-1 underline-offset-2"
										>{mod.Author}</span
									>
								</button>
							{:else}
								<p class="text-xs opacity-70">by {mod.Author}</p>
							{/if}
						</div>
					</div>
				</td>
				<td>
					<p class="font-semibold text-sm">{mod.Version}</p>
				</td>
				<td>
					<div class="flex items-center justify-end gap-1">
						<button
							class="btn btn-error btn-sm btn-square"
							disabled={isRemoving}
							onclick={() => onRemoveMod(mod.Name)}
						>
							<Trash2 size={14} />
						</button>
						<ModMenu {mod} {gamePath} />
					</div>
				</td>
			</tr>
		{/each}
	</tbody>
</table>
