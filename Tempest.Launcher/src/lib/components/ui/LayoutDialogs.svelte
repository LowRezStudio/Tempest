<script lang="ts">
	import InstanceWizard from "$lib/components/library/InstanceWizard.svelte";
	import InstallModOverlay from "$lib/components/mods/InstallModOverlay.svelte";
	import InstanceSelectModal from "$lib/components/mods/InstanceSelectModal.svelte";
	import ReplaceModDialog from "$lib/components/mods/ReplaceModDialog.svelte";
	import UnverifiedModDialog from "$lib/components/mods/UnverifiedModDialog.svelte";
	import OnboardingDialog from "$lib/components/onboarding/OnboardingDialog.svelte";
	import HostServerWizard from "$lib/components/server-list/HostServerWizard.svelte";
	import JoinServerWizard from "$lib/components/server-list/JoinServerWizard.svelte";
	import UpdateDialog from "$lib/components/updater/UpdateDialog.svelte";
	import {
		replaceDialogStore,
		resolveReplaceMod,
		resolveUnverifiedMod,
		unverifiedDialogStore,
	} from "$lib/mods/ui.svelte";
	import {
		hostServerWizardOpen,
		instanceWizardOpen,
		joinServerWizardOpen,
	} from "$lib/stores/ui.svelte";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		isDraggingFiles?: boolean;
		showInstanceSelect?: boolean;
		onselect: (instance: Instance) => void;
		oncancel: () => void;
	}

	let {
		isDraggingFiles = false,
		showInstanceSelect = $bindable(false),
		onselect,
		oncancel,
	}: Props = $props();
</script>

<InstallModOverlay visible={isDraggingFiles} />
<InstanceWizard bind:open={instanceWizardOpen.value} />
<HostServerWizard bind:open={hostServerWizardOpen.value} />
<JoinServerWizard bind:open={joinServerWizardOpen.value} />
<InstanceSelectModal bind:open={showInstanceSelect} {onselect} {oncancel} />
<ReplaceModDialog
	bind:open={replaceDialogStore.value.open}
	modName={replaceDialogStore.value.modName}
	onconfirm={() => resolveReplaceMod(true)}
	oncancel={() => resolveReplaceMod(false)}
/>
<UnverifiedModDialog
	bind:open={unverifiedDialogStore.value.open}
	modName={unverifiedDialogStore.value.modName}
	onconfirm={() => resolveUnverifiedMod(true)}
	oncancel={() => resolveUnverifiedMod(false)}
/>
<UpdateDialog />
<OnboardingDialog />
