<script lang="ts">
	import InstanceSettingsForm from "$lib/components/library/InstanceSettingsForm.svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		instance: Instance;
		open?: boolean;
	}

	let { instance, open = $bindable(false) }: Props = $props();
</script>

<Modal bind:open title={m.instance_instance_settings()} class="max-w-2xl">
	<InstanceSettingsForm
		{instance}
		active={open}
		onSaved={() => (open = false)}
		onCancel={() => (open = false)}
	/>
</Modal>
