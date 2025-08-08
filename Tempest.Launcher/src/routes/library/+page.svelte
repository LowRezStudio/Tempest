<script>
	import { addInstance, instances } from "$lib/state/instances.svelte";
	import { Gamepad2, X } from "@lucide/svelte";
	import { createDialog, melt } from "@melt-ui/svelte";
	import { fade } from "svelte/transition";

	const {
		elements: {
			trigger,
			overlay,
			content,
			title,
			description,
			close,
			portalled,
		},
		states: { open },
	} = createDialog({
		forceVisible: true,
	});
</script>

<button
	use:melt={$trigger}
	class="
		inline-flex items-center justify-center rounded-xl bg-white px-4 py-3
		font-medium leading-none text-magnum-700 shadow hover:opacity-75
	"
>
	Open Dialog
</button>

{#if $open}
	<div class="" use:melt={$portalled}>
		<div
			use:melt={$overlay}
			class="fixed inset-0 z-50 bg-black/50"
			transition:fade={{ duration: 150 }}
		/>
		<div
			class="
				fixed left-1/2 top-1/2 z-50 max-h-[85vh] w-[90vw]
				max-w-[450px] -translate-x-1/2 -translate-y-1/2 rounded-xl bg-white
				p-6 shadow-lg
			"
			use:melt={$content}
		>
			<h2 use:melt={$title} class="m-0 text-lg font-medium text-black">
				Edit profile
			</h2>
			<p use:melt={$description} class="mb-5 mt-2 leading-normal text-zinc-600">
				Make changes to your profile here. Click save when you're done.
			</p>

			<fieldset class="mb-4 flex items-center gap-5">
				<label class="w-[90px] text-right text-black" for="name"> Name </label>
				<input
					class="
						inline-flex h-8 w-full flex-1 items-center justify-center
						rounded-sm border border-solid px-3 leading-none text-black
					"
					id="name"
					value="Thomas G. Lopes"
				/>
			</fieldset>
			<fieldset class="mb-4 flex items-center gap-5">
				<label class="w-[90px] text-right text-black" for="username">
					Username
				</label>
				<input
					class="
						inline-flex h-8 w-full flex-1 items-center justify-center
						rounded-sm border border-solid px-3 leading-none text-black
					"
					id="username"
					value="@thomasglopes"
				/>
			</fieldset>
			<div class="mt-6 flex justify-end gap-4">
				<button
					use:melt={$close}
					class="
						inline-flex h-8 items-center justify-center rounded-sm
						bg-zinc-100 px-4 font-medium leading-none text-zinc-600
					"
				>
					Cancel
				</button>
				<button
					use:melt={$close}
					class="
						inline-flex h-8 items-center justify-center rounded-sm
						bg-blue-100 px-4 font-medium leading-none text-blue-900
					"
				>
					Save changes
				</button>
			</div>
			<button
				use:melt={$close}
				aria-label="close"
				class="
					absolute right-4 top-4 inline-flex h-6 w-6 appearance-none
					items-center justify-center rounded-full p-1 text-blue-800
					hover:bg-blue-100 focus:shadow-blue-400
				"
			>
				<X class="size-4" />
			</button>
		</div>
	</div>
{/if}

<div class="p-3">
	<div class="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] w-full gap-3 mr-auto scroll-smooth overflow-y-auto">
		{#each instances.current as instance}
			<a
				class="flex items-center justify-center p-3 gap-3 rounded-xl bg-[#16161a] hover:bg-[#1c1c21]"
				href={`/library/${instance.id}`}
			>
				<div class="grid place-items-center">
					<div class="grid place-items-center bg-slate-300 rounded-xl size-12">
						<img class="size-10" src="/img/crystal.png" alt="logo" />
					</div>
				</div>
				<div class="flex flex-col justify-between flex-auto">
					<p class="font-bold">{instance.label}</p>
					<div class="flex items-center gap-1.5 font-[550] text-[0.85rem] text-[#96a2b0]">
						<Gamepad2 size="20" />
						<p class="">{instance.version ?? "Unknown"}</p>
					</div>
				</div>
			</a>
		{/each}
	</div>
</div>
