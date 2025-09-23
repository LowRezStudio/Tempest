<script lang="ts">
	import { instances } from "$lib/state/instances.svelte";
	import { Box, Gamepad2 } from "@lucide/svelte";
</script>

<section class="h-full w-full overflow-auto">
	<div class="mx-auto max-w-6xl px-6 py-6">
		{#if instances.current.length == 0}
			<div
				class="border-background-800 from-background-950 to-background-900/60 grid place-items-center rounded-2xl border bg-gradient-to-b p-10"
			>
				<div class="max-w-xl text-center">
					<div
						class="bg-background-800 text-background-300 mx-auto grid size-14 place-items-center rounded-full"
					>
						<Box class="size-7" />
					</div>
					<p class="text-background-200 mt-4 text-base font-medium">
						Your library is empty.
					</p>
				</div>
			</div>
		{/if}
		<div class="grid grid-cols-[repeat(auto-fill,minmax(18rem,1fr))] gap-4">
			{#each instances.current as instance}
				<a
					href={`/library/${instance.id}`}
					class="group border-background-800 bg-background-950 hover:bg-background-900 flex items-center gap-4 rounded-2xl border px-4 py-4 transition-colors"
				>
					<div
						class="bg-background-900 border-background-700 text-primary-300 grid size-14 place-items-center rounded-2xl border-2 shadow-inner"
					>
						<Box class="size-7" />
					</div>
					<div class="min-w-0 flex-auto">
						<p class="truncate font-semibold tracking-tight">{instance.label}</p>
						<div class="mt-1 flex flex-wrap items-center gap-2 text-sm">
							<span
								class="border-background-700 bg-background-900 text-background-200 inline-flex items-center gap-2 rounded-lg border px-2 py-1"
							>
								<Gamepad2 class="text-secondary-300 size-4" />
								<span>{instance.version ?? "Unknown"}</span>
							</span>
						</div>
					</div>
				</a>
			{/each}
		</div>
	</div>
</section>
