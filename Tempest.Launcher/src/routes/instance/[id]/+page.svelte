<script lang="ts">
	import { page } from "$app/state";
	import { instanceMap } from "$lib/stores/instance";
	import {
		Play,
		Settings,
		EllipsisVertical,
		Gamepad2,
		RefreshCw,
		Trash2,
		Box,
	} from "@lucide/svelte";

	type ModItem = {
		id: string;
		name: string;
		author: string;
		version: string;
		enabled: boolean;
		icon?: string;
	};

	let activeTab = $state<"content">("content");

	// Mock data
	const mods = $state<ModItem[]>([
		{
			id: "1",
			name: "Multiplayer",
			author: "Tempest",
			version: "1.0.0",
			enabled: true,
		},
	]);

	const instance = instanceMap.get()[page.params.id!];

	if (!instance) throw new Error("how did we get here?");
</script>

<div class="flex flex-col h-full bg-base-100">
	<!-- Header -->
	<div>
		<div class="px-4 py-3">
			<div class="flex items-center justify-between">
				<!-- Left: Icon and Info -->
				<div class="flex items-center gap-3">
					<div
						class="w-16 h-16 rounded-xl bg-base-200 flex items-center justify-center shrink-0"
					>
						<Box size={32} class="opacity-60" />
					</div>
					<div>
						<h1 class="text-2xl font-bold mb-1">
							{instance?.label || "Cobblemon"}
						</h1>
						<div class="flex items-center gap-3 text-sm text-base-content/70">
							<div class="flex items-center gap-1.5">
								<Gamepad2 size={14} />
								<span>{instance?.version}</span>
							</div>
						</div>
					</div>
				</div>

				<!-- Right: Action Buttons -->
				<div class="flex items-center gap-2">
					<button class="btn btn-accent text-sm">
						<Play size={16} />
						Play
					</button>
					<button class="btn btn-square">
						<Settings size={16} />
					</button>
					<button class="btn btn-square">
						<EllipsisVertical size={16} />
					</button>
				</div>
			</div>
		</div>

		<!-- Tabs -->
		<div class="px-4">
			<div role="tablist" class="tabs tabs-border">
				<button
					role="tab"
					class={activeTab === "content" ? "tab tab-active" : "tab"}
					onclick={() => (activeTab = "content")}
				>
					Content
				</button>
			</div>
		</div>
	</div>

	<!-- Content Area -->
	<div class="flex-1 flex flex-col overflow-hidden bg-base-200">
		{#if activeTab === "content"}
			<!-- Mod List -->
			<div class="flex-1 overflow-y-auto">
				<div class="px-4">
					<table class="table">
						<thead>
							<tr>
								<!-- <th class="w-12">
									<input type="checkbox" class="checkbox checkbox-xs" />
								</th> -->
								<th>
									<button class="flex items-center gap-1 font-semibold text-sm">
										<span>Name</span>
									</button>
								</th>
								<th class="w-48">Version</th>
								<th class="w-auto text-right">
									<button class="btn btn-ghost btn-sm">
										<RefreshCw size={14} />
										Refresh
									</button>
								</th>
							</tr>
						</thead>
						<tbody>
							{#each mods as mod (mod.id)}
								<tr class="hover">
									<!-- <td>
										<input type="checkbox" class="checkbox checkbox-xs" />
									</td> -->
									<td>
										<div class="flex items-center gap-3">
											<div
												class="w-10 h-10 rounded-lg bg-base-300 flex items-center justify-center shrink-0"
											>
												<Box size={20} class="opacity-60" />
											</div>
											<div class="flex-1 min-w-0">
												<h3 class="font-bold text-sm truncate">
													{mod.name}
												</h3>
												<p class="text-xs opacity-70">by {mod.author}</p>
											</div>
										</div>
									</td>
									<td>
										<p class="font-semibold text-sm">{mod.version}</p>
									</td>
									<td>
										<div class="flex items-center justify-end gap-1">
											<button class="btn btn-error btn-sm btn-square">
												<Trash2 size={14} />
											</button>
											<button class="btn btn-sm btn-square">
												<EllipsisVertical size={14} />
											</button>
										</div>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</div>
		{/if}
	</div>
</div>
