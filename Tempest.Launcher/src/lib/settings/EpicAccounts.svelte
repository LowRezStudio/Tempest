<script lang="ts">
	import { Trash2, UserRoundPlus } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import {
		createEpicAccountsQuery,
		createEpicRemoveMutation,
		createEpicSelectMutation,
	} from "$lib/queries/epic";
	import { openEpicLogin } from "$lib/stores/epic.svelte";
	import type { EpicAccount } from "$ipc/contract";

	const accountsQuery = createEpicAccountsQuery();
	const select = createEpicSelectMutation();
	const remove = createEpicRemoveMutation();

	const list = $derived(accountsQuery.data);
	const accounts = $derived(list?.accounts ?? []);
	// The row whose change is still running.
	const busy = $derived(
		select.isPending ? select.variables : remove.isPending ? remove.variables : null,
	);
	const error = $derived((accountsQuery.error ?? select.error ?? remove.error)?.message ?? "");

	// Whether the account's refresh token is still good, when Epic said when it ends.
	const usable = (account: EpicAccount) =>
		account.refreshExpiresAt === null || account.refreshExpiresAt > Date.now();
</script>

<div class="flex flex-col gap-3">
	{#if accounts.length > 0}
		<div class="flex flex-col gap-2">
			{#each accounts as account (account.accountId)}
				{@const selected = list?.selectedId === account.accountId}
				<div
					class={[
						"bg-base-100 rounded-field flex items-center gap-3 border px-3 py-2",
						selected ? "border-primary ring-primary/20 ring-2" : "border-base-300",
					]}
				>
					<input
						id={`epic-account-${account.accountId}`}
						type="radio"
						class="radio radio-sm shrink-0"
						name="epic-default-account"
						checked={selected}
						disabled={!!busy}
						aria-label={m.settings_epic_use_account({ name: account.displayName })}
						onchange={() => select.mutate(account.accountId)}
					/>
					<label
						for={`epic-account-${account.accountId}`}
						class="min-w-0 flex-1 cursor-pointer"
					>
						<p class="flex items-center gap-2 text-sm font-medium">
							<span class="truncate">{account.displayName}</span>
							{#if selected}
								<span class="badge badge-ghost badge-xs shrink-0"
									>{m.settings_epic_default()}</span
								>
							{/if}
							{#if !usable(account)}
								<span class="badge badge-warning badge-xs shrink-0"
									>{m.settings_epic_expired()}</span
								>
							{/if}
						</p>
						<p class="text-base-content/60 truncate font-mono text-xs">
							{account.accountId}
						</p>
					</label>
					<button
						type="button"
						class="btn btn-ghost btn-xs text-error shrink-0"
						disabled={!!busy}
						onclick={() => remove.mutate(account.accountId)}
					>
						{#if busy === account.accountId}
							<span class="loading loading-spinner loading-xs"></span>
						{:else}
							<Trash2 size={12} />
						{/if}
						{m.settings_epic_remove()}
					</button>
				</div>
			{/each}
		</div>
	{:else if list}
		<p class="text-base-content/60 text-xs">{m.settings_epic_empty()}</p>
	{/if}

	<div class="flex items-center gap-2">
		<button type="button" class="btn btn-sm" onclick={() => openEpicLogin()}>
			<UserRoundPlus size={14} />
			{m.settings_epic_add()}
		</button>
	</div>

	{#if error}<p class="text-error text-xs">{error}</p>{/if}
</div>
