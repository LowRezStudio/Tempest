<script lang="ts">
	import { Trash2 } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";

	export type DeleteMode = "library" | "library_mods" | "delete";

	interface Props {
		open: boolean;
		instanceName: string;
		onconfirm: (mode: DeleteMode) => Promise<void> | void;
	}

	let { open = $bindable(), instanceName: _instanceName, onconfirm }: Props = $props();

	let selected: DeleteMode = $state("library");
	let isDeleting = $state(false);

	async function handleConfirm() {
		isDeleting = true;
		try {
			await onconfirm(selected);
		} finally {
			isDeleting = false;
			open = false;
		}
	}

	$effect(() => {
		if (!open) {
			selected = "library";
			isDeleting = false;
		}
	});
</script>

<Modal bind:open title={m.delete_title()} onsubmit={handleConfirm}>
	<div class="space-y-4">
		<div class="flex flex-col gap-2">
			<label
				class="flex cursor-pointer items-start gap-3 rounded-lg border p-3 transition-colors"
				class:border-accent={selected === "library"}
				class:bg-base-200={selected === "library"}
				class:border-base-300={selected !== "library"}
			>
				<input
					type="radio"
					class="radio radio-sm radio-accent mt-0.5"
					bind:group={selected}
					value="library"
				/>
				<div class="flex flex-col">
					<span class="label-text text-sm font-medium">{m.delete_option_library()}</span>
					<span class="text-xs opacity-60">{m.delete_option_library_hint()}</span>
				</div>
			</label>

			<label
				class="flex cursor-pointer items-start gap-3 rounded-lg border p-3 transition-colors"
				class:border-accent={selected === "library_mods"}
				class:bg-base-200={selected === "library_mods"}
				class:border-base-300={selected !== "library_mods"}
			>
				<input
					type="radio"
					class="radio radio-sm radio-accent mt-0.5"
					bind:group={selected}
					value="library_mods"
				/>
				<div class="flex flex-col">
					<span class="label-text text-sm font-medium"
						>{m.delete_option_library_mods()}</span
					>
					<span class="text-xs opacity-60">{m.delete_option_library_mods_hint()}</span>
				</div>
			</label>

			<label
				class="flex cursor-pointer items-start gap-3 rounded-lg border p-3 transition-colors"
				class:border-accent={selected === "delete"}
				class:bg-base-200={selected === "delete"}
				class:border-base-300={selected !== "delete"}
			>
				<input
					type="radio"
					class="radio radio-sm radio-error mt-0.5"
					bind:group={selected}
					value="delete"
				/>
				<div class="flex flex-col">
					<span class="label-text text-error text-sm font-medium"
						>{m.delete_option_delete()}</span
					>
					<span class="text-error/70 text-xs">{m.delete_option_delete_hint()}</span>
				</div>
			</label>
		</div>
	</div>

	{#snippet actions()}
		<button
			class="btn btn-ghost"
			type="button"
			onclick={() => (open = false)}
			disabled={isDeleting}
		>
			{m.common_cancel()}
		</button>
		<button class="btn btn-error" type="submit" disabled={isDeleting}>
			{#if isDeleting}
				<span class="loading loading-spinner loading-sm"></span>
				{m.common_deleting()}
			{:else}
				<Trash2 size={16} />
				{m.common_delete()}
			{/if}
		</button>
	{/snippet}
</Modal>
