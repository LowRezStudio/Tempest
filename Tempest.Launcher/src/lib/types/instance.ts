export type DownloadingState = {
	type: "downloading";
};

export type PausedState = {
	type: "paused";
};

export type PreparedState = {
	type: "prepared";
};

export type SetupState = {
	type: "setup";
};

export type InstanceState = DownloadingState | PausedState | SetupState | PreparedState;

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
	color?: string;
};
