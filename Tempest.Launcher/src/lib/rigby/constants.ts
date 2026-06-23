export const RIGBY_BASE_URL = "https://rigby.kyi.ro/chunks";
export const RIGBY_MANIFEST_URL_TEMPLATE = "https://rigby.kyi.ro/manifests/{version}.manifest.json";
export const WIKI_BASE_URL = "https://paladins.fandom.com/wiki/";

export function isPre20Version(version: string): boolean {
	return version.startsWith("0.") || version.startsWith("1.");
}
