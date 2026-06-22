<script lang="ts">
	import { AlertTriangle, ShieldAlert } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";

	interface Props {
		open: boolean;
		modName: string;
		onconfirm: () => void;
		oncancel: () => void;
	}

	let { open = $bindable(false), modName, onconfirm, oncancel }: Props = $props();

	function handleConfirm() {
		onconfirm();
		open = false;
	}

	function handleCancel() {
		oncancel();
		open = false;
	}
</script>

<Modal bind:open title={m.unverified_mod_title()} class="max-w-md" onsubmit={handleConfirm}>
	<div class="space-y-4">
		<div class="flex items-start gap-3">
			<div class="text-warning mt-0.5 shrink-0">
				<ShieldAlert size={24} />
			</div>
			<div>
				<h4 class="font-bold text-base">
					{m.unverified_mod_heading()}
				</h4>
				<p class="text-sm opacity-70 mt-1">
					{m.unverified_mod_message({ name: modName })}
				</p>
			</div>
		</div>
		<p class="text-sm opacity-60">
			{m.unverified_mod_warning()}
		</p>
	</div>

	{#snippet actions()}
		<button class="btn btn-ghost" type="button" onclick={handleCancel}>
			{m.common_cancel()}
		</button>
		<button class="btn btn-error" type="submit">
			<AlertTriangle size={16} />
			{m.unverified_mod_btn()}
		</button>
	{/snippet}
</Modal>
