<script lang="ts">
	import { KeyRound, ShieldCheck } from "@lucide/svelte";
	import { generateSigningKey, isKeyExistsError, signText } from "$lib/core/mods";
	import { promptSigningKeyId } from "$lib/mods/signing.svelte";
	import { m } from "$lib/paraglide/messages";
	import { invoke } from "$lib/platform/api";
	import { addToast } from "$lib/stores/ui.svelte";

	let hasKey = $state(false);
	let keyId = $state<string | null>(null);
	let keyBusy = $state(false);
	let keyError = $state("");

	let showVerify = $state(false);
	let verifyText = $state("");
	let verifyBusy = $state(false);
	let verifyError = $state("");
	let verifySignature = $state("");

	async function load() {
		try {
			const status = await invoke("signing:status");
			hasKey = status.hasKey;
			keyId = status.keyId;
		} catch (error) {
			console.error("Failed to load signing key status:", error);
		}
	}

	$effect(() => {
		void load();
	});

	// Asks for the key name in the shared dialog, then generates/overwrites.
	async function createKey() {
		keyError = "";
		const id = await promptSigningKeyId();
		if (!id) return;
		keyBusy = true;
		try {
			await generateSigningKey(id);
			await load();
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			// A key already being present is fine — show the existing one.
			if (isKeyExistsError(message)) {
				await load();
			} else {
				keyError = message;
			}
		} finally {
			keyBusy = false;
		}
	}

	// Signs the account-page verification text with the default key; the user
	// pastes the resulting signature back on the website to prove key ownership.
	async function verifyKey() {
		const text = verifyText.trim();
		if (!text || verifyBusy) return;
		verifyBusy = true;
		verifyError = "";
		verifySignature = "";
		try {
			verifySignature = await signText(text);
		} catch (error) {
			verifyError = error instanceof Error ? error.message : String(error);
		} finally {
			verifyBusy = false;
		}
	}

	async function copySignature() {
		if (!verifySignature) return;
		try {
			await navigator.clipboard.writeText(verifySignature);
			addToast({
				title: m.settings_signing_key_verify_copied(),
				message: m.settings_signing_key_verify_copied_message(),
				tone: "success",
			});
		} catch (error) {
			console.error("Failed to copy signature:", error);
		}
	}
</script>

<div class="flex flex-col gap-3">
	<p class="text-base-content/60 text-sm">{m.settings_signing_key_hint()}</p>

	<div class="bg-base-100 border-base-300 rounded-field border px-3 py-2 text-sm">
		<div class="flex flex-wrap items-center gap-2">
			<KeyRound size={14} class={hasKey ? "text-primary" : "opacity-60"} />
			{#if hasKey}
				<span class="min-w-0 flex-1 truncate font-mono" title={keyId ?? undefined}>
					{keyId ?? m.settings_signing_key_present()}
				</span>
				<button
					type="button"
					class={["btn btn-xs", showVerify ? "btn-primary btn-soft" : "btn-ghost"]}
					aria-expanded={showVerify}
					onclick={() => (showVerify = !showVerify)}
				>
					<ShieldCheck size={12} />
					{m.settings_signing_key_verify()}
				</button>
				<button
					type="button"
					class="btn btn-ghost btn-xs"
					disabled={keyBusy}
					onclick={createKey}
				>
					{m.settings_signing_key_replace()}
				</button>
			{:else}
				<span class="text-base-content/60 flex-1">{m.settings_signing_key_none()}</span>
				<button
					type="button"
					class="btn btn-primary btn-xs"
					disabled={keyBusy}
					onclick={createKey}
				>
					{m.converter_signing_key_create()}
				</button>
			{/if}
		</div>

		{#if showVerify && hasKey}
			<div class="border-base-300 mt-2 flex flex-col gap-2 border-t pt-2">
				<p class="text-base-content/70 text-xs">{m.settings_signing_key_verify_hint()}</p>
				<form
					class="flex items-center gap-2"
					onsubmit={(e) => {
						e.preventDefault();
						void verifyKey();
					}}
				>
					<input
						type="text"
						class="input input-sm min-w-0 flex-1"
						placeholder={m.settings_signing_key_verify_text_placeholder()}
						bind:value={verifyText}
					/>
					<button
						class="btn btn-primary btn-sm"
						disabled={verifyBusy || !verifyText.trim()}
					>
						{#if verifyBusy}<span class="loading loading-spinner loading-xs"
							></span>{/if}
						{m.settings_signing_key_verify_sign()}
					</button>
				</form>
				{#if verifyError}<p class="text-error text-xs">{verifyError}</p>{/if}
				{#if verifySignature}
					<textarea
						class="textarea textarea-sm h-32 w-full font-mono text-xs"
						readonly
						value={verifySignature}></textarea>
					<button type="button" class="btn btn-sm self-end" onclick={copySignature}>
						{m.settings_signing_key_verify_copy()}
					</button>
				{/if}
			</div>
		{/if}
	</div>

	{#if keyBusy}
		<p class="text-warning flex items-center gap-1 text-xs">
			<span class="loading loading-spinner loading-xs"></span>
			{m.converter_signing()}
		</p>
	{:else if keyError}
		<p class="text-error text-xs">{keyError}</p>
	{:else if hasKey}
		<p class="text-base-content/50 text-xs">{m.settings_signing_key_replace_hint()}</p>
	{/if}
</div>
