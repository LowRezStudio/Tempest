<script lang="ts">
	import { KeySquare } from "@lucide/svelte";
	import { repoLogin } from "$lib/core/mods";
	import { m } from "$lib/paraglide/messages";
	import { invoke } from "$lib/platform/api";

	let hasApiKey = $state(false);
	let maskedKey = $state<string | null>(null);
	let editing = $state(false);
	let apiKey = $state("");
	let busy = $state(false);
	let apiError = $state("");

	async function load() {
		try {
			const status = await invoke("repo:status");
			hasApiKey = status.hasApiKey;
			maskedKey = status.masked;
		} catch (error) {
			console.error("Failed to load API key status:", error);
		}
	}

	$effect(() => {
		void load();
	});

	async function save() {
		const key = apiKey.trim();
		if (!key || busy) return;
		busy = true;
		apiError = "";
		try {
			await repoLogin(key);
			apiKey = "";
			editing = false;
			await load();
		} catch (error) {
			apiError = error instanceof Error ? error.message : String(error);
		} finally {
			busy = false;
		}
	}

	async function forget() {
		try {
			await invoke("repo:forget");
			await load();
		} catch (error) {
			console.error("Failed to forget API key:", error);
		}
	}
</script>

<div class="flex flex-col gap-3">
	<p class="text-base-content/60 text-sm">{m.settings_api_key_hint()}</p>

	{#if hasApiKey && !editing}
		<div
			class="bg-base-100 border-base-300 rounded-field flex items-center gap-2 border px-3 py-2 text-sm"
		>
			<KeySquare size={14} class="text-primary" />
			<span class="min-w-0 flex-1 truncate font-mono">{maskedKey}</span>
			<button type="button" class="btn btn-ghost btn-xs" onclick={() => (editing = true)}>
				{m.settings_api_key_replace()}
			</button>
			<button type="button" class="btn btn-ghost btn-xs text-error" onclick={forget}>
				{m.settings_api_key_remove()}
			</button>
		</div>
	{:else}
		<form
			class="flex items-center gap-2"
			onsubmit={(e) => {
				e.preventDefault();
				void save();
			}}
		>
			<input
				type="password"
				class="input input-sm min-w-0 flex-1 font-mono"
				placeholder={m.converter_api_key_placeholder()}
				aria-label={m.settings_api_key_label()}
				bind:value={apiKey}
			/>
			{#if editing}
				<button
					type="button"
					class="btn btn-ghost btn-sm"
					onclick={() => (editing = false)}
				>
					{m.common_cancel()}
				</button>
			{/if}
			<button class="btn btn-primary btn-sm" disabled={busy || !apiKey.trim()}>
				{#if busy}<span class="loading loading-spinner loading-xs"></span>{/if}
				{busy ? m.converter_logging_in() : m.converter_login_btn()}
			</button>
		</form>
		{#if apiError}<p class="text-error text-xs">{apiError}</p>{/if}
	{/if}
</div>
