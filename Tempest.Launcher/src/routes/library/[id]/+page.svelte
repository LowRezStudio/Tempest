<script lang="ts">
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import Button from "$lib/components/ui/Button.svelte";
	import Dialog from "$lib/components/ui/Dialog.svelte";
	import * as core from "$lib/core";
	import { getInstance, processes, removeInstance } from "$lib/state/instances.svelte";
	import { Box, Gamepad2, Package, Settings } from "@lucide/svelte";

	const id = page.params.id ?? "";
	const instance = getInstance(id);

	if (!instance) throw new Error("how did we get here?");

	let activeTab = $state<"content" | "logs">("content");
	let settingsOpen = $state(false);

	const process = $derived(
		processes.value.find((p) => p.instance.id == id && p.mode == "client"),
	);

	// to-do: add confirmation prompt and ask if you should delete files
	const deleteInstance = () => {
		removeInstance(id);
		goto("/library");
	};

	core.getVersion().then((res) => console.log(res));

	const playGame = async () => {
		const cli = core.launchGame({
			path: instance.path,
			args: [...instance.launchOptions.args, { "-log": instance.launchOptions.log }],
			dllList: instance.launchOptions.dllList,
			noDefaultArgs: instance.launchOptions.noDefaultArgs,
		});

		cli.once("close", () => {
			processes.value = processes.value.filter(
				(p) => !(p.instance.id == id && p.mode == "client"),
			);
		});

		processes.value = [
			...processes.value,
			{
				instance,
				child: await cli.spawn(),
				mode: "client",
				start: new Date(),
			},
		];
	};
</script>

<Dialog bind:open={settingsOpen} title="Instance Settings">
	<Button kind="danger" onclick={deleteInstance}>Delete instance</Button>
</Dialog>

<section class="h-full w-full overflow-auto">
	<div class="mx-auto max-w-6xl px-6 py-6">
		<div class="flex items-center justify-between gap-4">
			<div class="flex items-center gap-4">
				<div
					class="bg-background-900 border-background-700 text-primary-300 grid size-16 place-items-center rounded-2xl border-2 shadow-inner"
				>
					<Box class="size-8" />
				</div>
				<div>
					<h1 class="text-2xl font-semibold tracking-tight">{instance.label}</h1>
					<div class="mt-1 flex flex-wrap items-center gap-3 text-sm">
						<span
							class="border-background-700 bg-background-900 text-background-200 inline-flex items-center gap-2 rounded-lg border px-2 py-1"
						>
							<Gamepad2 class="text-secondary-300 size-4" />
							<span>{instance.version ?? "Unknown"}</span>
						</span>
						<!-- <span class="inline-flex items-center gap-2 rounded-lg border border-background-700 bg-background-900 px-2 py-1 text-background-200">
							<Clock class="size-4 text-background-400" />
							<span>Never played</span>
						</span> -->
					</div>
				</div>
			</div>

			<div class="flex gap-4">
				<Button kind="accented" disabled={!!process} onclick={playGame}
					>{process ? "Playing..." : "Play"}</Button
				>
				<Button size="square" icon={Settings} onclick={() => (settingsOpen = true)} />
			</div>
		</div>

		<div class="border-background-800 mt-6 border-b">
			<nav class="flex gap-6">
				<button
					class="tab-btn"
					class:active={activeTab === "content"}
					onclick={() => (activeTab = "content")}
				>
					Content
				</button>
				<button
					class="tab-btn"
					class:active={activeTab === "logs"}
					onclick={() => (activeTab = "logs")}>Logs</button
				>
			</nav>
		</div>

		{#if activeTab === "content"}
			<div
				class="border-background-800 from-background-950 to-background-900/60 mt-8 grid place-items-center rounded-2xl border bg-gradient-to-b p-10"
			>
				<div class="max-w-xl text-center">
					<div
						class="bg-background-800 text-background-300 mx-auto grid size-14 place-items-center rounded-full"
					>
						<Package class="size-7" />
					</div>
					<p class="text-background-200 mt-4 text-base font-medium">
						You haven't added any content to this instance yet.
					</p>
				</div>
			</div>
		{:else if activeTab === "logs"}
			<div
				class="border-background-800 bg-background-950/60 text-background-300 mt-8 rounded-2xl border p-8"
			>
				TO-DO
			</div>
		{/if}
	</div>
</section>

<style>
	@reference "$lib/styles/global.css";

	.tab-btn {
		@apply text-background-300 hover:text-background-200 hover:border-background-600 relative -mb-px cursor-pointer border-b-2 border-transparent px-1 py-3 text-sm font-medium transition-colors;
	}
	.tab-btn.active {
		@apply border-primary-400 text-primary-300;
	}
</style>
