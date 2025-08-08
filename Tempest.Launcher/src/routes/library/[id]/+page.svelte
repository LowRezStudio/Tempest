<script lang="ts">
	import { page } from "$app/state";
	import { getInstance } from "$lib/state/instances.svelte";
	import { Bot, ChevronDown, Clock, Gamepad2, Box } from "@lucide/svelte";

	const id = page.params.id;
	const instance = getInstance(id);

	let activeTab = $state<"content" | "logs">("content");
</script>

{#if instance}
	<section class="w-full h-full overflow-auto">
		<div class="mx-auto max-w-6xl px-6 py-6">
			<div class="flex items-center justify-between gap-4">
				<div class="flex items-center gap-4">
					<div class="grid place-items-center size-16 rounded-2xl bg-background-900 border-2 border-background-700 text-primary-300 shadow-inner">
						<Box class="size-8" />
					</div>
					<div>
						<h1 class="text-2xl font-semibold tracking-tight">{instance.label}</h1>
						<div class="mt-1 flex flex-wrap items-center gap-3 text-sm">
							<span class="inline-flex items-center gap-2 rounded-lg border border-background-700 bg-background-900 px-2 py-1 text-background-200">
								<Gamepad2 class="size-4 text-secondary-300" />
								<span>{instance.version ?? "Unknown"}</span>
							</span>
							<!-- <span class="inline-flex items-center gap-2 rounded-lg border border-background-700 bg-background-900 px-2 py-1 text-background-200">
								<Clock class="size-4 text-background-400" />
								<span>Never played</span>
							</span> -->
						</div>
					</div>
				</div>

				<button class="inline-flex items-center gap-2 rounded-xl border border-green-600/40 bg-green-700/30 px-4 py-2 text-green-300 hover:bg-green-700/40 transition">
					<span class="relative flex size-2">
						<span class="absolute inline-flex size-full animate-ping rounded-full bg-green-400 opacity-75"></span>
						<span class="relative inline-flex size-2 rounded-full bg-green-400"></span>
					</span>
					Installing...
				</button>
			</div>

			<div class="mt-6 border-b border-background-800">
				<nav class="flex gap-6">
					<button class="tab-btn" class:active={activeTab === "content"} onclick={() => activeTab = "content"}>Content</button>
					<button class="tab-btn" class:active={activeTab === "logs"} onclick={() => activeTab = "logs"}>Logs</button>
				</nav>
			</div>

			{#if activeTab === "content"}
				<div class="mt-8 grid place-items-center rounded-2xl border border-background-800 bg-gradient-to-b from-background-950 to-background-900/60 p-10">
					<!-- <div class="text-center max-w-xl">
						<div class="mx-auto grid place-items-center size-14 rounded-full bg-background-800 text-background-300">
							<Bot class="size-7" />
						</div>
						<p class="mt-4 text-background-200 text-base font-medium">You haven't added any content to this instance yet.</p>
						<div class="mt-6 flex justify-center">
							<div class="inline-flex overflow-hidden rounded-xl border border-background-700">
								<button class="inline-flex items-center gap-2 bg-secondary-800/40 px-4 py-2 text-primary-200 hover:bg-secondary-700/40 transition">
									<span class="text-lg leading-none">+</span>
									<span>Install content</span>
								</button>
								<button class="grid place-items-center border-l border-background-700 bg-secondary-800/40 px-3 py-2 hover:bg-secondary-700/40 transition" aria-label="More options">
									<ChevronDown class="size-4 text-background-200" />
								</button>
							</div>
						</div>
					</div> -->
				</div>
			{:else if activeTab === "logs"}
				<div class="mt-8 rounded-2xl border border-background-800 bg-background-950/60 p-8 text-background-300">TO-DO</div>
			{/if}
		</div>
	</section>
{/if}

<style>
	@reference "../../../lib/styles/global.css";

	.tab-btn {
		@apply relative -mb-px px-1 py-3 text-sm font-medium border-b-2 transition-colors border-transparent text-background-300 cursor-pointer hover:text-background-200 hover:border-background-600;
	}
	.tab-btn.active {
		@apply border-primary-400 text-primary-300;
	}
</style>
