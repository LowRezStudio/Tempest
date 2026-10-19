interface ElectronAPI {
	invoke(channel: string, ...args: unknown[]): Promise<unknown>;
	on(channel: string, callback: (...args: unknown[]) => void): () => void;
	getPathForFile(file: File): string;
}

interface OSInfo {
	readonly platform: string;
	readonly arch: string;
	readonly type: string;
	readonly version: string;
}

interface Window {
	electronAPI?: ElectronAPI;
	__closeHook?: () => Promise<boolean>;
	__os?: OSInfo;
}
