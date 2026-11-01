<script lang="ts">
	import { AlertTriangle, ShieldAlert } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";

	interface Props {
		open: boolean;
		modName: string;
		signer: string;
		keyListOffline: boolean;
		/** From the mod repository: reviewed by moderators, only the signer's key is unvetted. */
		fromRepository?: boolean;
		onconfirm: () => void;
		oncancel: () => void;
	}

	let {
		open = $bindable(false),
		modName,
		signer,
		keyListOffline,
		fromRepository = false,
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
	title={fromRepository ? m.unvetted_signer_title() : m.unverified_mod_title()}
	class="max-w-md"
	onclose={handleCancel}
>
	<div class="space-y-4">
		<div class="flex items-start gap-3">
			<div class="text-warning mt-0.5 shrink-0">
				<ShieldAlert size={24} />
			</div>
			<div>
				<h4 class="text-base font-bold">
					{fromRepository ? m.unvetted_signer_heading() : m.unverified_mod_heading()}
				</h4>
				<p class="mt-1 text-sm opacity-70">
					{fromRepository
						? m.unvetted_signer_message({ name: modName })
						: m.unverified_mod_message({ name: modName })}
				</p>
				{#if signer}
					<p class="mt-1 font-mono text-xs break-all opacity-60">
						{m.unverified_mod_signer({ signer })}
					</p>
				{/if}
			</div>
		</div>
		<p class="text-sm opacity-60">
			{fromRepository ? m.unvetted_signer_warning() : m.unverified_mod_warning()}
		</p>
		{#if keyListOffline}
			<p class="text-warning text-sm">
				{m.unverified_mod_key_list_offline()}
			</p>
		{/if}
	</div>

	{#snippet actions()}
		<button
			class={["btn", fromRepository ? "btn-warning" : "btn-error"]}
			type="button"
			onclick={handleConfirm}
		>
			<AlertTriangle size={16} />
			{fromRepository ? m.unvetted_signer_btn() : m.unverified_mod_btn()}
		</button>
	{/snippet}
</Modal>
