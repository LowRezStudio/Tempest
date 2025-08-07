<script lang="ts">
	import cross from "@iconify-icons/mdi/close";
	import Icon from "@iconify/svelte";
	let { showModal = $bindable(), header, children } = $props();

	let dialog = $state();

	$effect(() => {
		if (showModal){
			dialog.showModal();
		} else {
			dialog.close();
		}
	});
</script>

<dialog
	bind:this={dialog}
	onclose={() => 
		(showModal = false)}
	onclick={(e) => {
		if (e.target === dialog) {
			dialog.close();
		}
	}}
>
	<div class="header">
		<h3>{header}</h3>
		<button id="closeBtn" onclick={() => dialog.close()}>
			<Icon icon={cross} style="font-size: 24px;"></Icon>
		</button>
	</div>
	<hr>
	<div class="content">
		{@render children?.()}
	</div>
	<hr>
</dialog>

<style>
	.header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 15rem;
		padding: 1.75rem;
	}
	#closeBtn {
		all: unset;
		border-radius: 100%;
		background-color: rgb(34, 35, 41);
		padding: 7px;
		font-size: 24px;
		color: rgb(144, 152, 161);
	}
	#closeBtn:hover {
		background-color: rgb(42, 44, 51);
		cursor: pointer;
	}
	dialog {
		position: absolute;
		left: 50%;
		top: 50%;
		transform: translate(-50%, -50%);
		border-radius: 15px;
		border: none;
		padding: 0;
		margin: 0;
		background-color: rgba(16, 16, 19, 1);
		color: white;
	}

	dialog {
		animation: fade 0.5s cubic-bezier(0.34, 1.56, 0.64, 1);
	}

	dialog[open]::backdrop {
		animation: fade 0.2s ease-out;
		backdrop-filter: blur(6px);
		background: rgba(0, 0, 0, 0.3);
	}
	@keyframes fade {
		from {
			opacity: 0;
		}
		to {
			opacity: 1;
		}
	}
	hr {
		height: 2px;
		background-color: rgb(34, 35, 41);
		border: none;
	}
</style>
