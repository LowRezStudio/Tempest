<script lang="ts">
	import { AlertTriangle, Replace } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";

	interface Props {
		open: boolean;
		modName: string;
		isModConflict?: boolean;
		onconfirm: () => void;
		oncancel: () => void;
	}

	let {
		open = $bindable(false),
		modName,
		isModConflict = true,
		onconfirm,
		oncancel,
	}: Props = $props();

	function handleConfirm() {
		onconfirm();
		open = false;
	}

	function handleCancel() {
		oncancel();
		open = false;
	}
</script>

<Modal
	bind:open
	title={isModConflict ? m.conflict_mod_title() : m.conflict_file_title()}
	class="max-w-md"
>
	<div class="space-y-4">
		<div class="flex items-start gap-3">
			<div class="text-warning mt-0.5 shrink-0">
				<AlertTriangle size={24} />
			</div>
			<div>
				<h4 class="font-bold text-base">
					{isModConflict ?
						m.conflict_replace_mod_heading()
					:	m.conflict_replace_file_heading()}
				</h4>
				{#if isModConflict}
					<p class="text-sm opacity-70 mt-1">
						{m.conflict_mod_message({ name: modName })}
					</p>
				{:else}
					<p class="text-sm opacity-70 mt-1">
						{m.conflict_file_message({ name: modName })}
					</p>
				{/if}
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
			{isModConflict ? m.conflict_replace_mod_btn() : m.conflict_replace_file_btn()}
		</button>
	{/snippet}
</Modal>
