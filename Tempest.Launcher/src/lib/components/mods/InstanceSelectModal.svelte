<script lang="ts">
	import { PackageX } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { getInstanceColor } from "$lib/utils/color";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		open: boolean;
		onselect: (instance: Instance) => void;
		oncancel: () => void;
	}

	let { open = $bindable(false), onselect, oncancel }: Props = $props();

	// Filter only instances that are in prepared state
	let instances = $derived(
		Object.values(instanceMap.value).filter((inst) => inst && inst.state?.type === "prepared") as Instance[],
	);

	function handleSelect(inst: Instance) {
		onselect(inst);
		open = false;
	}
</script>

<Modal bind:open title={m.select_instance_title()} class="max-w-md">
	<div class="space-y-4">
		<p class="text-sm opacity-70">
			{m.select_instance_hint()}
		</p>

		{#if instances.length === 0}
			<div class="alert alert-warning">
				<PackageX size={20} />
				<span>{m.select_instance_no_instances()}</span>
			</div>
		{:else}
			<ul class="list bg-base-100 rounded-box shadow-md max-h-60 overflow-y-auto">
				{#each instances as inst (inst.id)}
					<li>
						<button
							type="button"
							class="list-row hover:bg-base-200 w-full text-left cursor-pointer transition-colors"
							onclick={() => handleSelect(inst)}
						>
							<div
								class="w-12 h-12 rounded-lg flex items-center justify-center shrink-0 overflow-hidden"
								style="background-color: {getInstanceColor(inst)}"
							>
								<img src="/img/crystal.png" alt="" class="w-9 h-9 object-contain" />
							</div>
							<div class="list-col-grow min-w-0">
								<h4 class="font-bold text-base truncate mb-0.5">{inst.label}</h4>
								<div class="flex items-center gap-2 text-xs">
									{#if inst.version}
										<span class="opacity-60 font-mono shrink-0">
											{inst.version}
										</span>
									{/if}
									<span class="opacity-50 truncate">{inst.path}</span>
								</div>
							</div>
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</Modal>
