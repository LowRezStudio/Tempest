// GamePlatform is the host-side union (electron/src/ipc/contract.ts); the
// const is validated against it at compile time so renderer and host can't
// drift apart.
import type { GamePlatform } from "../../../electron/src/ipc/contract";

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

export const instancePlatforms = ["Win64", "Win32"] as const satisfies readonly GamePlatform[];

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
	manifestId?: string;
	appId?: number;
	path: string;
	launchOptions: InstanceLaunchOptions;
	state: InstanceState;
	color?: string;
};
