<script lang="ts">
	import { instances } from "$lib/state/instances.svelte";
	import { Box, Gamepad2 } from "@lucide/svelte";
</script>

<section class="w-full h-full overflow-auto">
	<div class="mx-auto max-w-6xl px-6 py-6">
		<div class="grid grid-cols-[repeat(auto-fill,minmax(18rem,1fr))] gap-4">
			{#each instances.current as instance}
				<a
					href={`/library/${instance.id}`}
					class="group flex items-center gap-4 rounded-2xl border border-background-800 bg-background-950 px-4 py-4 transition-colors hover:bg-background-900"
				>
					<div
						class="grid place-items-center size-14 rounded-2xl bg-background-900 border-2 border-background-700 text-primary-300 shadow-inner"
					>
						<Box class="size-7" />
					</div>
					<div class="min-w-0 flex-auto">
						<p class="font-semibold tracking-tight truncate">{instance.label}</p>
						<div class="mt-1 flex flex-wrap items-center gap-2 text-sm">
							<span
								class="inline-flex items-center gap-2 rounded-lg border border-background-700 bg-background-900 px-2 py-1 text-background-200"
							>
								<Gamepad2 class="size-4 text-secondary-300" />
								<span>{instance.version ?? "Unknown"}</span>
							</span>
						</div>
					</div>
				</a>
			{/each}
		</div>
	</div>
</section>
