import { persistedState } from "./persisted.svelte";

export type FlagValue = boolean | string | number;

export interface FlagDefinition<T extends FlagValue = FlagValue> {
	description: string;
	default: T;
}

export const FEATURE_FLAGS = {
	"public-servers": {
		description: "Show the public Internet servers tab on the Servers page.",
		default: false,
	},
} as const satisfies Record<string, FlagDefinition<FlagValue>>;

export type FlagName = keyof typeof FEATURE_FLAGS;

type WidenLiteral<T> = T extends boolean
	? boolean
	: T extends string
		? string
		: T extends number
			? number
			: T;

export type FlagValueOf<K extends FlagName> = WidenLiteral<(typeof FEATURE_FLAGS)[K]["default"]>;

type BooleanFlagName = {
	[K in FlagName]: (typeof FEATURE_FLAGS)[K]["default"] extends boolean ? K : never;
}[FlagName];

export type Flags = Record<string, FlagValue>;

export const flags = persistedState<Flags>("featureFlags", {});

export const isFlagEnabled = <K extends BooleanFlagName>(name: K): boolean => {
	const value = flags.value[name];
	return value === true || value === "true" || value === 1;
};

export const getFlag = <K extends FlagName>(name: K, fallback?: FlagValueOf<K>): FlagValueOf<K> => {
	const value = flags.value[name];
	return (value ?? fallback ?? FEATURE_FLAGS[name].default) as FlagValueOf<K>;
};

export interface FeatureFlagAPI {
	enable: (name: BooleanFlagName) => void;
	disable: (name: BooleanFlagName) => void;
	set: <K extends FlagName>(name: K, value: FlagValueOf<K>) => void;
	unset: (name: FlagName) => void;
	list: () => Flags;
	reset: () => void;
	isEnabled: (name: BooleanFlagName) => boolean;
}

if (typeof window !== "undefined") {
	const log = (msg: string, ...args: unknown[]) => console.info(`[flags] ${msg}`, ...args);

	(window as unknown as { __flags: FeatureFlagAPI }).__flags = {
		enable(name) {
			flags.set({ ...flags.value, [name]: true });
			log(`enabled "${name}"`);
		},
		disable(name) {
			flags.set({ ...flags.value, [name]: false });
			log(`disabled "${name}"`);
		},
		set(name, value) {
			flags.set({ ...flags.value, [name]: value });
			log(`set "${name}"`, value);
		},
		unset(name) {
			const next = { ...flags.value };
			delete next[name];
			flags.set(next);
			log(`unset "${name}"`);
		},
		list() {
			console.table(flags.value);
			return { ...flags.value };
		},
		reset() {
			flags.set({});
			log("reset all flags");
		},
		isEnabled(name) {
			return isFlagEnabled(name);
		},
	};
}
