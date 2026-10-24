<script lang="ts">
	import InstanceWizard from "$lib/components/library/InstanceWizard.svelte";
	import LegacyTauriDialog from "$lib/components/migration/LegacyTauriDialog.svelte";
	import DependenciesDialog from "$lib/components/mods/DependenciesDialog.svelte";
	import InstanceSelectModal from "$lib/components/mods/InstanceSelectModal.svelte";
	import ReplaceModDialog from "$lib/components/mods/ReplaceModDialog.svelte";
	import UnverifiedModDialog from "$lib/components/mods/UnverifiedModDialog.svelte";
	import HostServerWizard from "$lib/components/server-list/HostServerWizard.svelte";
	import JoinServerWizard from "$lib/components/server-list/JoinServerWizard.svelte";
	import AntivirusBlockedDialog from "$lib/components/ui/AntivirusBlockedDialog.svelte";
	import UpdateDialog from "$lib/components/updater/UpdateDialog.svelte";
	import {
		dependenciesDialogStore,
		instancePickerStore,
		replaceDialogStore,
		resolveDependencies,
		resolvePickInstance,
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
		showInstanceSelect?: boolean;
		onselect: (instance: Instance) => void;
		oncancel: () => void;
	}

	let { showInstanceSelect = $bindable(false), onselect, oncancel }: Props = $props();
</script>

<InstanceWizard bind:open={instanceWizardOpen.value} />
<HostServerWizard bind:open={hostServerWizardOpen.value} />
<JoinServerWizard bind:open={joinServerWizardOpen.value} />
<InstanceSelectModal bind:open={showInstanceSelect} {onselect} {oncancel} />
<InstanceSelectModal
	bind:open={instancePickerStore.value.open}
	compat={instancePickerStore.value.request?.compat}
	hint={instancePickerStore.value.request?.hint}
	onselect={(inst) => resolvePickInstance(inst)}
	oncancel={() => resolvePickInstance(null)}
/>
<ReplaceModDialog
	bind:open={replaceDialogStore.value.open}
	modName={replaceDialogStore.value.modName}
	newModName={replaceDialogStore.value.newModName}
	conflictingMods={replaceDialogStore.value.conflictingMods}
	onconfirm={(choice) => resolveReplaceMod(choice)}
	oncancel={() => resolveReplaceMod("cancel")}
/>
<UnverifiedModDialog
	bind:open={unverifiedDialogStore.value.open}
	modName={unverifiedDialogStore.value.modName}
	signer={unverifiedDialogStore.value.signer}
	keyListOffline={unverifiedDialogStore.value.keyListOffline}
	onconfirm={() => resolveUnverifiedMod(true)}
	oncancel={() => resolveUnverifiedMod(false)}
/>
<DependenciesDialog
	bind:open={dependenciesDialogStore.value.open}
	modName={dependenciesDialogStore.value.modName}
	items={dependenciesDialogStore.value.items}
	onconfirm={(choice) => resolveDependencies(choice)}
	oncancel={() => resolveDependencies("cancel")}
/>
<UpdateDialog />
<LegacyTauriDialog />
<AntivirusBlockedDialog />
