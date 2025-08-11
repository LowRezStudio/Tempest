<script lang="ts">
	import Button from "$lib/components/ui/Button.svelte";
	import Dialog from "$lib/components/ui/Dialog.svelte";
	import Select from "$lib/components/ui/Select.svelte";
	import versions from "$lib/data/versions.json";
	import { Plus } from "@lucide/svelte";

	let open = $state(false);

	const versionItems = Object.entries(versions).flatMap(([group, list]) =>
		list.map((item) => ({
			value: item.id,
			label: `${item.version} - ${item.date}`,
			group: group,
		}))
	);

	let selectedVersion = $state<string | undefined>();
</script>

<button
	onclick={() => open = !open}
	class="grid place-items-center size-10 rounded-xl text-gray-400 transition-all duration-300 ease-in-out transform translate-z-0 hover:text-primary-300 hover:scale-102 active:scale-98 cursor-pointer"
>
	<Plus class="size-5 transition-transform duration-200 ease-in-out" />
</button>

<Dialog bind:open title="Adding an instance">
	<div class="flex flex-col gap-4 items-center">
		<div class="flex items-center justify-center gap-2">
			<Button class="text-text-color font-semibold">Download</Button>
			<Button class="text-text-color font-semibold">From Folder</Button>
		</div>
		<hr class="border-background-700 w-full" />
		<Select type="single" items={versionItems} bind:value={selectedVersion} placeholder="Choose a version" />
	</div>
</Dialog>
