const UNITS = ["B", "KB", "MB", "GB", "TB"];

export const folderName = (path: string): string =>
	path.split(/[/\\]/).filter(Boolean).pop() ?? path;

export function formatBytes(bytes: number): string {
	if (!(bytes >= 1)) return "0 B";
	const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), UNITS.length - 1);
	return `${Number.parseFloat((bytes / 1024 ** i).toFixed(1))} ${UNITS[i]}`;
}

export const formatSpeed = (bytesPerSecond: number): string => `${formatBytes(bytesPerSecond)}/s`;

export function formatDuration(seconds: number): string {
	const s = Math.round(seconds);
	if (s < 60) return `${s}s`;
	if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
	return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}
