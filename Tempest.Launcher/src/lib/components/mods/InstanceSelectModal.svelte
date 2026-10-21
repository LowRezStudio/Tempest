<script lang="ts">
	import { PackageX } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import PaladinsIcon from "$lib/components/ui/PaladinsIcon.svelte";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import { isCompatible } from "$lib/utils/version";
	import type { RepoCompat } from "$lib/queries/repo";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		open: boolean;
		onselect: (instance: Instance) => void;
		oncancel: () => void;
		/** Lists instances on these patches first and marks the others. */
		compat?: RepoCompat | null;
		hint?: string;
	}

	let { open = $bindable(false), onselect, oncancel, compat, hint }: Props = $props();

	const fits = (inst: Instance) => !compat || isCompatible(compat, inst.version);

	let instances = $derived(
		(
			Object.values(instanceMap.value).filter(
				(inst) => inst && inst.state?.type === "prepared",
			) as Instance[]
		).sort((a, b) => Number(fits(b)) - Number(fits(a))),
	);

	function handleSelect(inst: Instance) {
		onselect(inst);
		open = false;
	}
</script>

<Modal bind:open title={m.select_instance_title()} class="max-w-md" onclose={oncancel}>
	<div class="space-y-4">
		<p class="text-sm opacity-70">
			{hint ?? m.select_instance_hint()}
		</p>

		{#if instances.length === 0}
			<div class="alert alert-warning">
				<PackageX size={20} />
				<span>{m.select_instance_no_instances()}</span>
			</div>
		{:else}
			<ul class="list bg-base-100 rounded-box max-h-60 overflow-y-auto shadow-md">
				{#each instances as inst (inst.id)}
					{@const ok = fits(inst)}
					<li>
						<button
							type="button"
							class="list-row hover:bg-base-200 w-full cursor-pointer text-left transition-colors"
							class:opacity-50={!ok}
							onclick={() => handleSelect(inst)}
						>
							<div
								class="flex h-12 w-12 shrink-0 items-center justify-center overflow-hidden rounded-lg"
								style="background-color: {getInstanceColor(inst)}"
							>
								<PaladinsIcon
									size={40}
									color={getContrastColor(getInstanceColor(inst))}
								/>
							</div>
							<div class="list-col-grow min-w-0">
								<h4 class="mb-0.5 truncate text-base font-bold">{inst.label}</h4>
								<div class="flex items-center gap-2 text-xs">
									{#if inst.version}
										<span class="shrink-0 font-mono opacity-60">
											{inst.version}
										</span>
									{/if}
									{#if !ok}
										<span class="badge badge-warning badge-xs shrink-0">
											{m.repo_incompatible({ patch: inst.version ?? "" })}
										</span>
									{/if}
									<span class="truncate opacity-50">{inst.path}</span>
								</div>
							</div>
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</Modal>
