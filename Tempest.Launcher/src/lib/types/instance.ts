export type UnpreparedState = {
	type: "unprepared";
	status: "downloading" | "paused";
	percentage: number;
};

export type PreparedState = {
	type: "prepared";
};

export type InstanceState = UnpreparedState | PreparedState;

export type InstanceLaunchOptions = {
	dllList: string[];
	args: string[];
	noDefaultArgs: boolean;
	log: boolean;
};

export type Instance = {
	id: string;
	label: string;
	version?: string;
	path: string;
	launchOptions: InstanceLaunchOptions;
	state: InstanceState;
};
