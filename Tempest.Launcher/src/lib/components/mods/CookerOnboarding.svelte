<script lang="ts">
	import { CheckCircle2, KeyRound } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { generateSigningKey, isKeyExistsError, repoLogin } from "$lib/core/mods";
	import { m } from "$lib/paraglide/messages";
	import { invoke } from "$lib/platform/api";
	import { isFlagEnabled } from "$lib/stores/flags.svelte";

	interface Props {
		open?: boolean;
		onclose?: () => void;
	}

	let { open = $bindable(false), onclose }: Props = $props();

	const publishingEnabled = $derived(isFlagEnabled("mod-publishing"));

	let step = $state(1);
	let loading = $state(true);

	let hasKey = $state(false);
	let keyId = $state<string | null>(null);
	let keyName = $state("");
	let keyBusy = $state(false);
	let keyError = $state("");

	let hasApiKey = $state(false);
	let maskedKey = $state<string | null>(null);
	let apiKey = $state("");
	let apiBusy = $state(false);
	let apiError = $state("");

	// Load statuses each time the dialog opens.
	$effect(() => {
		if (!open) return;
		step = 1;
		keyError = "";
		apiError = "";
		keyName = "";
		apiKey = "";
		loading = true;
		void (async () => {
			try {
				const [signing, repo] = await Promise.all([
					invoke("signing:status"),
					invoke("repo:status"),
				]);
				hasKey = signing.hasKey;
				keyId = signing.keyId;
				hasApiKey = repo.hasApiKey;
				maskedKey = repo.masked;
			} catch (error) {
				console.error("Failed to load publishing setup status:", error);
			} finally {
				loading = false;
			}
		})();
	});

	async function createKey() {
		const id = keyName.trim();
		if (!id || keyBusy) return;
		keyBusy = true;
		keyError = "";
		try {
			await generateSigningKey(id);
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			// A key already being present is fine: show it and move on.
			if (!isKeyExistsError(message)) {
				keyError = message;
				return;
			}
		} finally {
			keyBusy = false;
		}
		const status = await invoke("signing:status");
		hasKey = status.hasKey;
		keyId = status.keyId;
	}

	async function saveApiKey() {
		const key = apiKey.trim();
		if (!key || apiBusy) return;
		apiBusy = true;
		apiError = "";
		try {
			await repoLogin(key);
			apiKey = "";
			const status = await invoke("repo:status");
			hasApiKey = status.hasApiKey;
			maskedKey = status.masked;
		} catch (error) {
			apiError = error instanceof Error ? error.message : String(error);
		} finally {
			apiBusy = false;
		}
	}

	function finish() {
		open = false;
		// Every exit counts as done: never nag again — everything here is
		// changeable later in Settings.
		onclose?.();
	}

	function handleClose() {
		finish();
	}
</script>

<Modal bind:open onclose={handleClose} title={m.cooker_onboarding_title()} class="max-w-md">
	{#if loading}
		<div class="flex items-center justify-center py-10">
			<span class="loading loading-spinner loading-lg opacity-30"></span>
		</div>
	{:else if step === 1}
		<p class="text-sm opacity-70">{m.cooker_onboarding_intro()}</p>

		<div class="mt-4">
			<div class="flex items-center gap-2">
				<KeyRound size={16} />
				<span class="text-sm font-semibold">{m.cooker_onboarding_key_title()}</span>
			</div>
			<p class="mt-1 text-xs opacity-60">{m.cooker_onboarding_key_hint()}</p>

			{#if hasKey}
				<div class="bg-success/10 mt-3 rounded-lg px-3 py-2">
					<div class="text-success flex items-center gap-2 text-sm font-medium">
						<CheckCircle2 size={16} class="shrink-0" />
						{m.cooker_onboarding_key_ready()}
					</div>
					{#if keyId}
						<p class="mt-0.5 truncate font-mono text-xs opacity-60">{keyId}</p>
					{/if}
				</div>
			{:else}
				<input
					type="text"
					class="input input-bordered mt-3 w-full"
					bind:value={keyName}
					placeholder={m.converter_signing_key_placeholder()}
					spellcheck="false"
					onkeydown={(e) => e.key === "Enter" && keyName.trim() && createKey()}
				/>
				{#if keyError}
					<p class="text-error mt-1 text-xs">{keyError}</p>
				{/if}
				<button
					class="btn btn-primary mt-3 w-full"
					disabled={keyBusy || !keyName.trim()}
					onclick={createKey}
				>
					{#if keyBusy}
						<span class="loading loading-spinner loading-xs"></span>
					{/if}
					{m.converter_signing_key_create()}
				</button>
			{/if}
		</div>

		<div class="mt-4 flex items-center justify-between">
			<button class="btn btn-ghost btn-sm" onclick={finish}>
				{m.cooker_onboarding_skip_setup()}
			</button>
			<button
				class="btn btn-primary btn-sm"
				onclick={() => (step = publishingEnabled ? 2 : 3)}
			>
				{m.cooker_onboarding_next()}
			</button>
		</div>
	{:else if step === 2 && publishingEnabled}
		<div>
			<div class="flex items-center gap-2">
				<KeyRound size={16} />
				<span class="text-sm font-semibold">{m.cooker_onboarding_api_title()}</span>
			</div>
			<p class="mt-1 text-xs opacity-60">{m.cooker_onboarding_api_hint()}</p>

			{#if hasApiKey}
				<div class="bg-success/10 mt-3 rounded-lg px-3 py-2">
					<div class="text-success flex items-center gap-2 text-sm font-medium">
						<CheckCircle2 size={16} class="shrink-0" />
						{m.cooker_onboarding_api_ready()}
					</div>
					{#if maskedKey}
						<p class="mt-0.5 font-mono text-xs opacity-60">{maskedKey}</p>
					{/if}
				</div>
			{:else}
				<input
					type="password"
					class="input input-bordered mt-3 w-full"
					bind:value={apiKey}
					placeholder={m.converter_api_key_placeholder()}
					onkeydown={(e) => e.key === "Enter" && apiKey.trim() && saveApiKey()}
				/>
				{#if apiError}
					<p class="text-error mt-1 text-xs">{apiError}</p>
				{/if}
				<button
					class="btn btn-primary mt-3 w-full"
					disabled={apiBusy || !apiKey.trim()}
					onclick={saveApiKey}
				>
					{#if apiBusy}
						<span class="loading loading-spinner loading-xs"></span>
					{/if}
					{m.converter_login_btn()}
				</button>
				<button class="btn btn-ghost btn-sm mt-2 w-full" onclick={() => (step = 3)}>
					{m.cooker_onboarding_api_skip()}
				</button>
			{/if}
		</div>

		<div class="mt-4 flex items-center justify-between">
			<button class="btn btn-ghost btn-sm" onclick={finish}>
				{m.cooker_onboarding_skip_setup()}
			</button>
			{#if hasApiKey}
				<button class="btn btn-primary btn-sm" onclick={() => (step = 3)}>
					{m.cooker_onboarding_next()}
				</button>
			{/if}
		</div>
	{:else}
		<div class="flex flex-col items-center gap-3 py-4 text-center">
			<CheckCircle2 size={40} class="text-success" />
			<p class="text-base font-bold">{m.cooker_onboarding_done_title()}</p>
			<p class="text-sm opacity-70">{m.cooker_onboarding_done_text()}</p>
		</div>
		<button class="btn btn-primary w-full" onclick={finish}>
			{m.cooker_onboarding_finish()}
		</button>
	{/if}
</Modal>
