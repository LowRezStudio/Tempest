import { persistedJSON } from "./persisted.svelte";

/** Where a tab sits: above the first divider, either end of the instances' section, or under the second divider. */
export const sidebarZones = ["top", "middleTop", "middleBottom", "bottom"] as const;
export type SidebarZone = (typeof sidebarZones)[number];

export type SidebarTab =
	| "onboarding"
	| "home"
	| "library"
	| "downloads"
	| "servers"
	| "converter"
	| "lobby"
	| "logs"
	| "settings";

export type SidebarLayout = Record<SidebarZone, SidebarTab[]>;

const defaultLayout: SidebarLayout = {
	top: ["onboarding", "home", "library", "downloads", "servers", "converter", "lobby"],
	middleTop: [],
	middleBottom: [],
	bottom: ["logs", "settings"],
};

const stored = persistedJSON<Partial<SidebarLayout>>("sidebarLayout", defaultLayout);

// Tabs a saved layout doesn't know yet go where they sit by default; ones it knows twice or
// that no longer exist are dropped.
function normalize(saved: Partial<SidebarLayout>): SidebarLayout {
	const known = new Set(Object.values(defaultLayout).flat());
	const seen = new Set<SidebarTab>();
	const layout = Object.fromEntries(
		sidebarZones.map((zone) => [
			zone,
			(Array.isArray(saved[zone]) ? saved[zone] : []).filter((tab) => {
				if (!known.has(tab) || seen.has(tab)) return false;
				seen.add(tab);
				return true;
			}),
		]),
	) as SidebarLayout;
	for (const zone of sidebarZones) {
		for (const tab of defaultLayout[zone]) if (!seen.has(tab)) layout[zone].push(tab);
	}
	return layout;
}

export const sidebarLayout = {
	get value(): SidebarLayout {
		return normalize(stored.value);
	},
	set value(layout: SidebarLayout) {
		stored.value = layout;
	},
};

// Moves `tab` to `index` among the tabs of `zone` that are shown. Hidden tabs (a feature that is
// off) keep their places, so they come back where they were.
export function moveSidebarTab(
	tab: SidebarTab,
	zone: SidebarZone,
	index: number,
	shown: (tab: SidebarTab) => boolean,
) {
	const layout = sidebarLayout.value;
	for (const z of sidebarZones) layout[z] = layout[z].filter((t) => t !== tab);
	const list = layout[zone];
	const visible = list.filter(shown);
	const before = visible[index];
	const at =
		before !== undefined
			? list.indexOf(before)
			: visible.length > 0
				? list.indexOf(visible.at(-1)!) + 1
				: list.length;
	list.splice(at, 0, tab);
	sidebarLayout.value = layout;
}

export function resetSidebarLayout() {
	stored.value = defaultLayout;
}
