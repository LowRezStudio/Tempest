<script lang="ts">
	import { KeyRound } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { settleSigningKeyPrompt, signingKeyPrompt } from "$lib/mods/signing.svelte";
	import { m } from "$lib/paraglide/messages";

	let id = $state("");

	function create() {
		const value = id.trim();
		id = "";
		settleSigningKeyPrompt(value || null);
	}

	function cancel() {
		id = "";
		settleSigningKeyPrompt(null);
	}
</script>

<Modal
	bind:open={signingKeyPrompt.open}
	onclose={cancel}
	title={m.converter_signing_key_title()}
	class="max-w-md"
>
	<div class="flex items-start gap-3">
		<div class="text-primary mt-0.5 shrink-0">
			<KeyRound size={24} />
		</div>
		<p class="text-sm opacity-70">{m.converter_signing_key_hint()}</p>
	</div>
	<input
		type="text"
		class="input input-bordered mt-4 w-full"
		bind:value={id}
		placeholder={m.converter_signing_key_placeholder()}
		spellcheck="false"
		onkeydown={(e) => e.key === "Enter" && id.trim() && create()}
	/>

	{#snippet actions()}
		<button class="btn btn-ghost" type="button" onclick={cancel}>{m.common_cancel()}</button>
		<button class="btn btn-primary" type="button" disabled={!id.trim()} onclick={create}>
			<KeyRound size={16} />
			{m.converter_signing_key_create()}
		</button>
	{/snippet}
</Modal>
