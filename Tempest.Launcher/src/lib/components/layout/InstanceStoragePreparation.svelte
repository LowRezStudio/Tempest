<script lang="ts">
	import { untrack } from "svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import {
		instanceStorage,
		prepareIndependentInstances,
	} from "$lib/core/instance-storage.svelte";
	import { instanceMap } from "$lib/stores/instance.svelte";

	$effect(() => {
		JSON.stringify(Object.values(instanceMap.value).map((i) => i && [i.id, i.path]));
		untrack(() => void prepareIndependentInstances().catch(() => {}));
	});
</script>

{#if instanceStorage.error}
	<div class="alert alert-error fixed inset-x-4 top-4 z-50" role="alert">
		<span>{instanceStorage.error}</span>
		<button
			class="btn btn-sm"
			onclick={() => void prepareIndependentInstances().catch(() => {})}
		>
			Retry instance preparation
		</button>
	</div>
{/if}
<Modal open={instanceStorage.busy} title="Preparing independent instances" dismissible={false}>
	<div class="flex items-center gap-3">
		<span class="loading loading-spinner loading-sm"></span>
		<p>{instanceStorage.label || "Checking game folders…"}</p>
	</div>
	<p class="mt-3 text-sm opacity-70">
		Shared game folders are copied so each instance has its own mods. This may take a while.
	</p>
</Modal>
