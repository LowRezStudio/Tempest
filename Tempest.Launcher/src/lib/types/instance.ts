export type UnpreparedState = {
	type: "unprepared";
	status: "downloading" | "paused";
	percentage: number;
};

export type PreparedState = {
	type: "prepared";
};

export type SetupState = {
	type: "setup";
};

export type InstanceState = UnpreparedState | SetupState | PreparedState;

export const instancePlatforms = ["Win64", "Win32"] as const;

export type InstancePlatform = (typeof instancePlatforms)[number];

export type InstanceLaunchOptions = {
	dllList: string[];
	args: string[];
	noDefaultArgs: boolean;
	log: boolean;
	platform?: InstancePlatform;
};

export type Instance = {
	id: string;
	label: string;
	version?: string;
	path: string;
	launchOptions: InstanceLaunchOptions;
	state: InstanceState;
};
