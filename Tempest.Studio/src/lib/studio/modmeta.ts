// What a mod's details must be, as the launcher's cooker, the CLI and the
// repository (mods.lowrezstudio.com) check them: the id, the version (semver
// without build metadata), dependency entries (`id` + an optional version
// requirement), the icon (a real PNG up to 1 MB) and the summary's length.

/** 2 to 64 of lowercase letters, digits, dots, dashes and underscores, starting with a letter or digit. */
export const MOD_ID_PATTERN = /^[a-z0-9][a-z0-9._-]{1,63}$/;

/** The longest summary the repository lists. */
export const MAX_SUMMARY = 140;

/** The largest icon a package may carry. */
export const MAX_ICON_BYTES = 1024 * 1024;

const SEMVER =
	/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?$/;

/** A package version: 1.2.3 or 1.2.3-beta.1 (no build metadata: nothing orders it). */
export const isSemver = (version: string): boolean => SEMVER.test(version);

/**
 * The semver nearest to what was typed ("v1.2" → "1.2.0", "1.0 beta_2" →
 * "1.0.0-beta.2"), or null when nothing in it reads as a version.
 * @param text What the user typed.
 * @returns A valid version, or null.
 */
export function toSemver(text: string): string | null {
	const m = /^\s*(?:v(?:ersion)?\s*)?(\d+)(?:[.,_](\d+))?(?:[.,_](\d+))?(?:[.,_]\d+)*(.*)$/i.exec(
		text.split("+")[0]!,
	);
	if (!m) return null;
	const num = (n: string) => n.replace(/^0+(?=\d)/, "");
	const core = [m[1]!, m[2] ?? "0", m[3] ?? "0"].map(num).join(".");
	const pre = m[4]!
		.split(/[^0-9A-Za-z]+/)
		.filter(Boolean)
		.map((p) => (/^\d+$/.test(p) ? num(p) : p));
	const version = pre.length > 0 ? `${core}-${pre.join(".")}` : core;
	return SEMVER.test(version) ? version : null;
}

/** The next patch version: 1.2.3 → 1.2.4 (a pre-release keeps its numbers: 1.2.3-beta.1 → 1.2.3). */
export function nextPatch(version: string): string {
	const m = /^(\d+)\.(\d+)\.(\d+)(-.+)?$/.exec(version);
	if (!m) return "1.0.1";
	return m[4] ? `${m[1]}.${m[2]}.${m[3]}` : `${m[1]}.${m[2]}.${Number(m[3]) + 1}`;
}

/** A mod id from a name: "My Champion!" → "my-champion". */
export function slugify(name: string): string {
	return name
		.toLowerCase()
		.normalize("NFKD")
		.replaceAll(/[\u0300-\u036f]/g, "")
		.replaceAll(/[^a-z0-9._-]+/g, "-")
		.replaceAll(/^[-._]+|[-._]+$/g, "")
		.slice(0, 64);
}

// One comparator of a requirement: `>=1.2`, `^1`, `~1.2.3`, `1.2.x`, a bare version…
const COMPARATOR =
	/^(>=|<=|>|<|==|=|\^|~)?\s*(\d+)(?:\.(\d+|x|\*))?(?:\.(\d+|x|\*))?(?:-([0-9A-Za-z.-]+))?$/;

function requirementValid(requirement: string): boolean {
	const text = requirement.trim();
	if (!text || text === "*") return true;
	return text.split("||").every((part) => {
		const tokens = part.split(/[\s,]+/).filter(Boolean);
		return (
			tokens.length > 0 &&
			tokens.every((t) => {
				const m = COMPARATOR.exec(t);
				// A pre-release needs the full version (1.2.3-beta, not 1.2-beta).
				return !!m && !(m[5] && (m[4] === undefined || /[x*]/.test(m[3] ?? "")));
			})
		);
	});
}

/**
 * Why a dependency line is not one the repository accepts, or null when it is:
 * a mod id (or `tempest`, the launcher) then an optional requirement such as
 * `>=1.0.0`, `^2`, `~1.2` or `>=1.0 <2.0`.
 * @param line One entry as typed.
 * @returns A message, or null.
 */
export function dependencyProblem(line: string): string | null {
	const m = /^([A-Za-z0-9._-]+)\s*(.*)$/s.exec(line.trim());
	if (!m) return `"${line.trim()}" does not start with a mod id`;
	const id = m[1]!.toLowerCase();
	if (id !== "tempest" && !MOD_ID_PATTERN.test(id)) return `"${m[1]}" is not a mod id`;
	const requirement = m[2]!.trim();
	if (requirement && (!/^[<>=^~*0-9]/.test(requirement) || !requirementValid(requirement))) {
		return `"${requirement}" is not a version requirement (for example >=1.0.0, ^2 or ~1.2)`;
	}
	return null;
}

const PNG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/**
 * Why an image cannot be a package's icon, or null when it can: packages carry
 * it as `icon.png`, so it must really be one (not a renamed JPEG), and small.
 * @param data The file's bytes.
 * @returns A message, or null.
 */
export function iconProblem(data: Uint8Array): string | null {
	if (!PNG.every((b, i) => data[i] === b)) return "The icon must be a PNG image.";
	if (data.length > MAX_ICON_BYTES) {
		return `The icon is ${(data.length / 1024 ** 2).toFixed(2)} MB; it may be 1 MB at most.`;
	}
	return null;
}

/** True when the repository refused the credentials (log in again, not a generic error). */
export const isAuthError = (message: string): boolean =>
	/no api key|unauthorized|401|403/i.test(message);

/** What `tempest mod publish --json` reports. */
export type PublishResult = {
	state: string;
	modId?: string;
	version?: string;
	errors: string[];
	warnings: string[];
};

/**
 * The outcome printed by `mod publish --json`, or null when stdout holds none.
 * @param stdout The CLI's output.
 * @returns The parsed result.
 */
export function parsePublish(stdout: string): PublishResult | null {
	try {
		const intent = JSON.parse(stdout.trim()) as {
			state?: string;
			result?: { modId?: string; version?: string; errors?: string[]; warnings?: string[] };
		};
		if (!intent.state) return null;
		return {
			state: intent.state,
			modId: intent.result?.modId,
			version: intent.result?.version,
			errors: intent.result?.errors ?? [],
			warnings: intent.result?.warnings ?? [],
		};
	} catch {
		return null;
	}
}
