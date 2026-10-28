export function bytes(n: number): string {
	if (n < 1024) return `${n} B`;
	if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
	if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
	return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function basename(path: string): string {
	const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
	return i >= 0 ? path.slice(i + 1) : path;
}

export function dirname(path: string): string {
	const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
	return i >= 0 ? path.slice(0, i) : path;
}

export function stem(path: string): string {
	const b = basename(path);
	const i = b.lastIndexOf(".");
	return i > 0 ? b.slice(0, i) : b;
}

export function joinPath(dir: string, name: string): string {
	const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
	return dir.endsWith(sep) ? dir + name : dir + sep + name;
}

/** `a.b.c` → `c` (object name from a path). */
export function leaf(objectPath: string): string {
	const i = objectPath.lastIndexOf(".");
	return i >= 0 ? objectPath.slice(i + 1) : objectPath;
}

/** A filesystem-safe file name for an object path. */
export function safeName(objectPath: string): string {
	return objectPath.replaceAll(/[/\\:*?"<>|]/g, "_");
}

export function ms(n: number | undefined): string {
	if (n === undefined) return "";
	return n < 1000 ? `${n} ms` : `${(n / 1000).toFixed(1)} s`;
}

/**
 * A path in a form to compare with another: forward slashes, no Windows
 * verbatim prefix (`\\?\`), lower case (Windows and macOS folders ignore case).
 * @param path The path.
 */
export function pathKey(path: string): string {
	return path
		.replaceAll("\\", "/")
		.replace(/^\/\/\?\//, "")
		.toLowerCase();
}

/**
 * Whether two paths name the same file (see `pathKey`).
 * @param a A path.
 * @param b Another path.
 */
export function samePath(a: string, b: string): boolean {
	return pathKey(a) === pathKey(b);
}
