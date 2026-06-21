<script lang="ts">
	import { AlertTriangle, Replace } from "@lucide/svelte";
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

<Modal bind:open title={m.conflict_mod_title()} class="max-w-md">
	<div class="space-y-4">
		<div class="flex items-start gap-3">
			<div class="text-warning mt-0.5 shrink-0">
				<AlertTriangle size={24} />
			</div>
			<div>
				<h4 class="font-bold text-base">
					{m.conflict_replace_mod_heading()}
				</h4>
				<p class="text-sm opacity-70 mt-1">
					{m.conflict_mod_message({ name: modName })}
				</p>
			</div>
		</div>
		<p class="text-sm opacity-60">
			{m.conflict_overwrite_warning()}
		</p>
	</div>

	{#snippet actions()}
		<button class="btn btn-ghost" onclick={handleCancel}> {m.common_cancel()} </button>
		<button class="btn btn-error" onclick={handleConfirm}>
			<Replace size={16} />
			{m.conflict_replace_mod_btn()}
		</button>
	{/snippet}
</Modal>
