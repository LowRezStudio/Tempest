<script lang="ts">
	import Button from "$lib/components/ui/Button.svelte";
	import { ensureLoaded, list, refresh } from "$lib/state/servers.svelte";
	import { onMount } from "svelte";

	onMount(ensureLoaded);
</script>

<h1>Servers</h1>
<div class="header-buttons">
	<Button onclick={refresh}>Refresh</Button>
	<Button href="/servers/create-server">Create Server</Button>
</div>

<table class="server-list">
	<tbody>
		<tr class="server-entry">
			<th class="server-name">I love kyiro Server</th>
			<td>OB 57</td>
			<td>Siege (Brightmarsh)</td>
			<td>0/10</td>
			<td class="button-row">
				<div class="button-container">
					<Button>Join</Button>
					<Button>Favorite</Button>
				</div>
			</td>
		</tr>
		<tr class="server-entry">
			<th class="server-name">I love kyiro Server</th>
			<td>OB 57</td>
			<td>Siege (Brightmarsh)</td>
			<td>0/10</td>
			<td class="button-row">
				<div class="button-container">
					<Button>Join</Button>
					<Button>Favorite</Button>
				</div>
			</td>
		</tr>
		<tr class="server-entry">
			<th class="server-name">I love kyiro Server</th>
			<td>OB 57</td>
			<td>Siege (Brightmarsh)</td>
			<td>0/10</td>
			<td class="button-row">
				<div class="button-container">
					<Button>Join</Button>
					<Button>Favorite</Button>
				</div>
			</td>
		</tr>
		<tr class="server-entry">
			<th class="server-name">I love kyiro Server</th>
			<td>OB 57</td>
			<td>Siege (Brightmarsh)</td>
			<td>0/10</td>
			<td class="button-row">
				<div class="button-container">
					<Button>Join</Button>
					<Button>Favorite</Button>
				</div>
			</td>
		</tr>
		<tr class="server-entry">
			<th class="server-name">I love kyiro Server</th>
			<td>OB 57</td>
			<td>Siege (Brightmarsh)</td>
			<td>0/10</td>
			<td class="button-row">
				<div class="button-container">
					<Button>Join</Button>
					<Button>Favorite</Button>
				</div>
			</td>
		</tr>
		<tr class="server-entry">
			<th class="server-name">I love kyiro Server</th>
			<td>OB 57</td>
			<td>Siege (Brightmarsh)</td>
			<td>0/10</td>
			<td class="button-row">
				<div class="button-container">
					<Button>Join</Button>
					<Button>Favorite</Button>
				</div>
			</td>
		</tr>
		<tr class="server-entry">
			<th class="server-name">I love kyiro Server</th>
			<td>OB 57</td>
			<td>Siege (Brightmarsh)</td>
			<td>0/10</td>
			<td class="button-row">
				<div class="button-container">
					<Button>Join</Button>
					<Button>Favorite</Button>
				</div>
			</td>
		</tr>
		<tr class="server-entry">
			<th class="server-name">I love kyiro Server</th>
			<td>OB 57</td>
			<td>Siege (Brightmarsh)</td>
			<td>0/10</td>
			<td class="button-row">
				<div class="button-container">
					<Button>Join</Button>
					<Button>Favorite</Button>
				</div>
			</td>
		</tr>
		<tr class="server-entry">
			<th class="server-name">I love kyiro Server</th>
			<td>OB 57</td>
			<td>Siege (Brightmarsh)</td>
			<td>0/10</td>
			<td class="button-row">
				<div class="button-container">
					<Button>Join</Button>
					<Button>Favorite</Button>
				</div>
			</td>
		</tr>

		{#each list as server}
			<tr class="server-entry">
				<th class="server-name">{server.name}</th>
				<td>{server.version}</td>
				<td>{server.map.length == 0 ? "Unknown" : server.map}</td>
				<td>{server.players}/{server.maxPlayers}</td>
				<td class="button-row">
					<div class="button-container">
						<Button>Join</Button>
						<Button>Favorite</Button>
					</div>
				</td>
			</tr>
		{/each}
	</tbody>
</table>

<style>
	.header-buttons {
		display: flex;
		justify-content: space-between;
	}
	table, tbody {
		border-collapse: separate;
		border-spacing: 0 var(--spacing-md);
		width: 100%;
	}

	.server-entry {
		background: linear-gradient(to top, var(--border-primary) 0%, transparent 75%);
		outline: 1px solid var(--border-primary);
		box-shadow:
			rgba(0, 0, 0, 0.3) 0px 3px 6px 0px,
			rgba(0, 0, 0, 0.1) 0px 3px 6px 0px;
		width: 100%;
		text-align: left;
	}

	.server-entry td, .server-entry th {
		padding: var(--spacing-sm) var(--spacing-md);
	}

	.server-name {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		width: 100%;
	}

	.button-row {
		width: 1%;
		white-space: nowrap;
	}

	.button-container {
		display: flex;
		justify-content: flex-end;
		gap: var(--spacing-sm);
	}

	.button-container :global(*) {
		max-width: 8rem;
	}
</style>
