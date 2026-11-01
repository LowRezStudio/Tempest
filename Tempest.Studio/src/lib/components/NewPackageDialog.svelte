<!--
	A new, empty package in the game folder (`package.create`): unsaved like
	any edit, kept in the project, and shipped whole by the mod cooker. It
	takes the header settings (versions, compression) of another package.
-->
<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let name = $state("");
	let template = $state("");
	let working = $state(false);

	const content = $derived(studio.packages.filter((p) => !p.script && !p.map && !p.new));
	const clean = $derived(name.trim().replace(/\.upk$/i, ""));
	const valid = $derived(/^[A-Za-z0-9_]{2,64}$/.test(clean));
	const taken = $derived(
		studio.packages.some((p) => p.name.toLowerCase() === clean.toLowerCase()),
	);

	$effect(() => {
		if (!open) return;
		name = "";
		const current = studio.pkg && content.find((p) => p.path === studio.pkg?.path);
		template = current?.path ?? content[0]?.path ?? "";
	});

	async function create(event: Event) {
		event.preventDefault();
		if (!valid || taken || !studio.workspace) return;
		working = true;
		try {
			const path = `${studio.workspace.dir}/${clean}.upk`;
			await rpc("package.create", { path, template: template || undefined });
			studio.markDirty(path);
			await studio.refreshPackages();
			await studio.openPackage(path);
			toasts.push(
				"success",
				"Add objects to it with the + button of the export tree. It is saved with your other edits and goes into the mod whole.",
				`${clean}.upk created`,
			);
			open = false;
		} catch (error) {
			toasts.error(error, "Could not create the package");
		} finally {
			working = false;
		}
	}
</script>

<Modal bind:open title="New package" width="max-w-md">
	<form onsubmit={create} class="space-y-3 text-sm">
		<label class="flex flex-col gap-1">
			<span class="text-xs opacity-70"
				>Name (the file is &lt;name&gt;.upk in the game folder)</span
			>
			<input
				class="input input-sm w-full font-mono {name && (!valid || taken)
					? 'input-error'
					: ''}"
				placeholder="MyMod_Assets"
				spellcheck="false"
				bind:value={name}
			/>
			{#if name && !valid}
				<span class="text-error text-xs">Letters, digits and underscores, 2 to 64.</span>
			{:else if taken}
				<span class="text-error text-xs">A package of that name already exists.</span>
			{/if}
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs opacity-70">Made like (versions and compression of)</span>
			<select class="select select-sm w-full font-mono" bind:value={template}>
				{#each content as p (p.path)}<option value={p.path}>{p.name}</option>{/each}
			</select>
		</label>
		<p class="text-xs opacity-60">
			The package is empty and unsaved until you save; the game loads it when another package
			or the game data refers to its objects.
		</p>
		<div class="flex justify-end gap-2">
			<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}
				>Cancel</button
			>
			<button class="btn btn-primary btn-sm" disabled={!valid || taken || working}
				>Create</button
			>
		</div>
	</form>
</Modal>
