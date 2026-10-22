<script lang="ts">
	import { ListX, PackageX, Trash2 } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";

	export type DeleteMode = "library" | "library_mods" | "delete";

	interface Props {
		open: boolean;
		instanceName: string;
		onconfirm: (mode: DeleteMode) => Promise<void> | void;
	}

	let { open = $bindable(), instanceName, onconfirm }: Props = $props();

	const options = [
		{
			mode: "library",
			icon: ListX,
			title: m.delete_option_library,
			hint: m.delete_option_library_hint,
		},
		{
			mode: "library_mods",
			icon: PackageX,
			title: m.delete_option_library_mods,
			hint: m.delete_option_library_mods_hint,
		},
		{
			mode: "delete",
			icon: Trash2,
			title: m.delete_option_delete,
			hint: m.delete_option_delete_hint,
		},
	] as const;

	let selected = $state<DeleteMode>("library");
	let typed = $state("");
	let isDeleting = $state(false);
	// Deleting the game files only goes ahead once the name is typed, like removing a mod on the website.
	const ready = $derived(selected !== "delete" || typed.trim() === instanceName.trim());

	async function handleConfirm() {
		if (!ready || isDeleting) return;
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
			typed = "";
			isDeleting = false;
		}
	});
</script>

<Modal
	bind:open
	title={m.delete_title_named({ name: instanceName })}
	class="max-w-md"
	onsubmit={handleConfirm}
>
	<div class="flex flex-col gap-2">
		{#each options as o (o.mode)}
			{@const Icon = o.icon}
			{@const active = selected === o.mode}
			{@const danger = o.mode === "delete"}
			<label
				class={[
					"bg-base-100 rounded-field flex cursor-pointer items-center gap-3 border p-3 transition-colors",
					active
						? danger
							? "border-error ring-error/20 ring-2"
							: "border-primary ring-primary/20 ring-2"
						: "border-base-300 hover:border-primary/50",
				]}
			>
				<span
					class={[
						"flex size-8 shrink-0 items-center justify-center rounded-full transition-colors",
						active
							? danger
								? "bg-error/15 text-error"
								: "bg-primary/15 text-primary"
							: "bg-base-300 text-base-content/60",
					]}><Icon size={16} /></span
				>
				<span class="min-w-0 flex-1">
					<span class={["block text-sm font-medium", danger && "text-error"]}
						>{o.title()}</span
					>
					<span class="text-base-content/60 block text-xs">{o.hint()}</span>
				</span>
				<input
					type="radio"
					name="delete-mode"
					class={["radio radio-sm shrink-0", danger ? "radio-error" : "radio-primary"]}
					bind:group={selected}
					value={o.mode}
					disabled={isDeleting}
				/>
			</label>
		{/each}
	</div>

	{#if selected === "delete"}
		<label class="mt-4 flex flex-col gap-1.5 text-sm">
			<span>{m.delete_type_name()}</span>
			<input
				type="text"
				class="input input-sm w-full"
				bind:value={typed}
				placeholder={instanceName}
				autocomplete="off"
				spellcheck="false"
				disabled={isDeleting}
			/>
		</label>
	{/if}

	{#snippet actions()}
		<button
			class="btn btn-ghost btn-sm"
			type="button"
			onclick={() => (open = false)}
			disabled={isDeleting}
		>
			{m.common_cancel()}
		</button>
		<button class="btn btn-error btn-sm" type="submit" disabled={isDeleting || !ready}>
			{#if isDeleting}
				<span class="loading loading-spinner loading-xs"></span>
				{m.common_deleting()}
			{:else}
				<Trash2 size={14} />
				{selected === "delete" ? m.common_delete() : m.common_remove()}
			{/if}
		</button>
	{/snippet}
</Modal>
