// While true, the settings.json snapshot interval in +layout.svelte must not
// write. Set by the settings page's "Clear All": the interval fires
// asynchronously over IPC and would otherwise re-dump localStorage into
// settings.json between the reset deleting the snapshot and clearing storage,
// resurrecting the state (which the startup migration then imports back).
export const snapshotPaused = $state({ value: false });
