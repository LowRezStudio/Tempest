<script lang="ts">
	import { ExternalLink, UserRound } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { createEpicLoginMutation } from "$lib/queries/epic";
	import { EPIC_LOGIN_URL } from "$lib/stores/epic.svelte";
	import { addToast } from "$lib/stores/ui.svelte";

	interface Props {
		open: boolean;
		/** Why a sign-in is needed; the dialog's usual line when absent. */
		note?: string;
	}

	let { open = $bindable(false), note }: Props = $props();

	const login = createEpicLoginMutation();
	let code = $state("");

	function submit() {
		const value = code.trim();
		if (!value || login.isPending) return;
		login.mutate(value, {
			onSuccess: (account) => {
				open = false;
				addToast({
					message: m.epic_login_success({ name: account.displayName }),
					tone: "success",
				});
			},
		});
	}

	// Nothing typed into a previous attempt survives reopening the dialog. A sign-in still
	// running keeps its state, so closing the dialog cannot drop its result.
	$effect(() => {
		if (!open && !login.isPending) {
			code = "";
			login.reset();
		}
	});
</script>

<Modal
	bind:open
	title={m.epic_login_title()}
	icon={UserRound}
	description={note ?? m.epic_login_description()}
	class="max-w-md"
	onsubmit={submit}
>
	<div class="flex flex-col gap-3">
		<button type="button" class="btn btn-sm self-start" onclick={() => openUrl(EPIC_LOGIN_URL)}>
			<ExternalLink size={14} />
			{m.epic_login_open_page()}
		</button>
		<label class="flex flex-col gap-1">
			<span class="text-base-content/60 text-xs">{m.epic_login_code_label()}</span>
			<input
				type="text"
				class="input input-sm w-full font-mono"
				spellcheck="false"
				autocomplete="off"
				bind:value={code}
				placeholder={m.epic_login_code_placeholder()}
			/>
		</label>
		<p class="text-base-content/60 text-xs">{m.epic_login_hint()}</p>
		{#if login.error}<p class="text-error text-xs">{login.error.message}</p>{/if}
	</div>

	{#snippet actions()}
		<button type="button" class="btn btn-ghost" onclick={() => (open = false)}>
			{m.common_cancel()}
		</button>
		<button type="submit" class="btn btn-primary" disabled={login.isPending || !code.trim()}>
			{#if login.isPending}<span class="loading loading-spinner loading-xs"></span>{/if}
			{login.isPending ? m.common_signing_in() : m.epic_login_submit()}
		</button>
	{/snippet}
</Modal>
