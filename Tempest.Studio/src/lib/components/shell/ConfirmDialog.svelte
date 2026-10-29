<!-- The one confirmation dialog of the Studio (`ui.confirm`): says what an
	action removes before it runs. Enter confirms, Escape cancels. -->
<script lang="ts">
	import { TriangleAlert } from "@lucide/svelte";
	import { tick } from "svelte";
	import { remember, ui } from "$lib/studio/ui.svelte";

	let confirmButton = $state<HTMLButtonElement | null>(null);
	let dontAsk = $state(false);
	$effect(() => {
		if (ui.asking) {
			dontAsk = false;
			void tick().then(() => confirmButton?.focus());
		}
	});
</script>

{#if ui.asking}
	{@const q = ui.asking}
	<div
		class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
		role="presentation"
		onmousedown={(e) => e.target === e.currentTarget && q.answer(false)}
		onkeydown={(e) => {
			if (e.key === "Escape") {
				e.preventDefault();
				q.answer(false);
			}
		}}
	>
		<div
			class="bg-base-200 border-base-300 w-[min(30rem,100%)] rounded-lg border p-4 shadow-2xl"
			role="alertdialog"
			aria-modal="true"
			aria-labelledby="confirm-title"
			aria-describedby="confirm-message"
		>
			<div class="flex items-start gap-3">
				{#if q.danger}<TriangleAlert size={20} class="text-warning mt-0.5 shrink-0" />{/if}
				<div class="min-w-0 flex-1">
					<h3 id="confirm-title" class="font-semibold">{q.title}</h3>
					<p id="confirm-message" class="mt-1 text-sm opacity-80">{q.message}</p>
					{#if q.items?.length}
						<ul
							class="bg-base-100 mt-2 max-h-40 overflow-y-auto rounded p-2 font-mono text-xs"
						>
							{#each q.items as item, i (i)}
								<li class="truncate" title={item}>{item}</li>
							{/each}
						</ul>
					{/if}
				</div>
			</div>
			{#if q.remember}
				<label class="mt-3 flex items-center gap-2 text-xs opacity-80">
					<input type="checkbox" class="checkbox checkbox-xs" bind:checked={dontAsk} />
					Don't ask me again
				</label>
			{/if}
			<div class="mt-4 flex flex-wrap items-center justify-end gap-2">
				<button class="btn btn-ghost btn-sm" onclick={() => q.answer(false)}
					>{q.cancel ?? "Cancel"}</button
				>
				{#if q.alt}
					<button class="btn btn-sm" onclick={() => q.answer("alt")}>{q.alt}</button>
				{/if}
				<button
					bind:this={confirmButton}
					class="btn btn-sm {q.danger ? 'btn-error' : 'btn-primary'}"
					onclick={() => {
						if (q.remember && dontAsk) remember(q.remember, true);
						q.answer(true);
					}}>{q.confirm}</button
				>
			</div>
		</div>
	</div>
{/if}
