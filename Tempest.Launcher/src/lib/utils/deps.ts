// Mod dependencies, `[mod].dependencies` in manifest.toml: one string per entry, the mod id
// then an optional version requirement glued to it ("tempest-scripting>=1.0.0"). The id
// `tempest` is the launcher itself. The grammar is the repository's (mods.lowrezstudio.com
// `src/lib/deps.ts`), also implemented by the CLI (SemverRange.cs) and the runtime (loader.rs);
// keep the four in step:
//
// - comparators `>=`, `>`, `<=`, `<`, `=` (or `==`), `^` (same major, or same minor below 1.0),
//   `~` (same minor); a bare version means `=`
// - a partial version fills in: `>=1.2` is `>=1.2.0`, `<2` is `<2.0.0`, `=1.2` and `~1.2` are
//   `>=1.2.0 <1.3.0`, `^1` is `>=1.0.0 <2.0.0`
// - comparators side by side (spaces or commas) must all hold; `||` separates alternatives
// - prereleases order by plain semver precedence

// The reserved dependency id for the launcher version.
export const TEMPEST_ID = "tempest";

export type Dependency = { id: string; requirement: string };

type Version = { nums: [number, number, number]; pre?: string };
type Comparator = { op: ">=" | ">" | "<=" | "<"; version: Version };

const MOD_ID = /^[a-z0-9][a-z0-9._-]{1,63}$/;
const PARTIAL = /^(\d+)(?:\.(\d+|x|\*))?(?:\.(\d+|x|\*))?(?:-([0-9A-Za-z.-]+))?$/;
const SEMVER =
	/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?$/;

// What a package version must be: 1.2.3 or 1.2.3-beta.1. Build metadata is left out, the
// comparers here and in the CLI don't order it.
export const isSemver = (version: string) => SEMVER.test(version);

// The semver nearest to what was typed ("v1.2" → "1.2.0", "1.0 beta_2" → "1.0.0-beta.2"), or
// null when nothing in it reads as a version.
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

// Semver precedence, prerelease-aware; non-semver strings compare as text. Negative when a < b.
export function compareSemver(a: string, b: string): number {
	const pa = SEMVER.exec(a);
	const pb = SEMVER.exec(b);
	if (!pa || !pb) return a.localeCompare(b);
	for (let i = 1; i <= 3; i++) {
		const d = Number(pa[i]) - Number(pb[i]);
		if (d !== 0) return d;
	}
	const ra = pa[4];
	const rb = pb[4];
	if (ra === rb) return 0;
	if (ra === undefined) return 1;
	if (rb === undefined) return -1;
	const xs = ra.split(".");
	const ys = rb.split(".");
	for (let i = 0; i < Math.max(xs.length, ys.length); i++) {
		const x = xs[i];
		const y = ys[i];
		if (x === undefined) return -1;
		if (y === undefined) return 1;
		const nx = /^\d+$/.test(x);
		const ny = /^\d+$/.test(y);
		if (nx && ny) {
			const d = Number(x) - Number(y);
			if (d !== 0) return d;
		} else if (nx !== ny) return nx ? -1 : 1;
		else if (x !== y) return x < y ? -1 : 1;
	}
	return 0;
}

// Splits "tempest-scripting>=1.0.0" into its id and requirement; null when the id is malformed.
export function parseDependency(text: string): Dependency | null {
	const m = /^([A-Za-z0-9._-]+)\s*(.*)$/s.exec(text.trim());
	if (!m) return null;
	const id = m[1]!.toLowerCase();
	if (id !== TEMPEST_ID && !MOD_ID.test(id)) return null;
	const requirement = m[2]!.trim();
	if (requirement && !/^[<>=^~*0-9]/.test(requirement)) return null;
	return { id, requirement };
}

const fmt = (v: Version) => `${v.nums.join(".")}${v.pre ? `-${v.pre}` : ""}`;

function bump(nums: [number, number, number], at: 0 | 1 | 2): Version {
	const out: [number, number, number] = [...nums];
	out[at] += 1;
	for (let i = at + 1; i < 3; i++) out[i] = 0;
	return { nums: out };
}

function parseComparator(token: string): Comparator[] | null {
	const m = /^(>=|<=|>|<|==|=|\^|~)?\s*(.+)$/.exec(token);
	if (!m) return null;
	const op = m[1] ?? "=";
	const p = PARTIAL.exec(m[2]!);
	if (!p) return null;
	const major = Number(p[1]);
	const minorRaw = p[2];
	const patchRaw = p[3];
	const minor =
		minorRaw === undefined || minorRaw === "x" || minorRaw === "*" ? null : Number(minorRaw);
	const patch =
		minor === null || patchRaw === undefined || patchRaw === "x" || patchRaw === "*"
			? null
			: Number(patchRaw);
	if (p[4] && patch === null) return null;
	const nums: [number, number, number] = [major, minor ?? 0, patch ?? 0];
	const low: Version = { nums, pre: p[4] };
	const ge = (v: Version): Comparator => ({ op: ">=", version: v });
	const lt = (v: Version): Comparator => ({ op: "<", version: v });
	const lineEnd = () => bump(nums, minor === null ? 0 : 1);
	if (op === ">=") return [ge(low)];
	if (op === "<") return [lt(low)];
	if (op === ">") return patch === null ? [ge(lineEnd())] : [{ op: ">", version: low }];
	if (op === "<=") return patch === null ? [lt(lineEnd())] : [{ op: "<=", version: low }];
	if (op === "=" || op === "==") {
		return patch === null ? [ge(low), lt(lineEnd())] : [ge(low), { op: "<=", version: low }];
	}
	if (op === "~") return [ge(low), lt(lineEnd())];
	if (op === "^") {
		if (minor === null || major > 0) return [ge(low), lt(bump(nums, 0))];
		if (patch === null || minor > 0) return [ge(low), lt(bump(nums, 1))];
		return [ge(low), lt(bump(nums, 2))];
	}
	return null;
}

function parseRequirement(requirement: string): Comparator[][] | null {
	const text = requirement.trim();
	if (!text || text === "*") return [[]];
	const alternatives: Comparator[][] = [];
	for (const part of text.split("||")) {
		const alt: Comparator[] = [];
		for (const token of part.split(/[\s,]+/).filter(Boolean)) {
			const comparators = parseComparator(token);
			if (!comparators) return null;
			alt.push(...comparators);
		}
		if (alt.length === 0 && part.trim() !== "") return null;
		alternatives.push(alt);
	}
	return alternatives;
}

const HOLDS: Record<Comparator["op"], (d: number) => boolean> = {
	">=": (d) => d >= 0,
	">": (d) => d > 0,
	"<=": (d) => d <= 0,
	"<": (d) => d < 0,
};

// Whether `version` satisfies `requirement`; an unparseable requirement matches nothing.
export function satisfies(version: string, requirement: string): boolean {
	const alternatives = parseRequirement(requirement);
	if (!alternatives) return false;
	return alternatives.some((alt) =>
		alt.every((c) => HOLDS[c.op](compareSemver(version, fmt(c.version)))),
	);
}

// "any version" for an empty requirement, else the requirement as written.
export const describeRequirement = (requirement: string) => requirement || "*";

// The launcher requirement among the dependencies, if any.
export const tempestRequirement = (deps: Dependency[]) =>
	deps.find((d) => d.id === TEMPEST_ID)?.requirement;

// Dependencies on other mods (everything but `tempest`).
export const modDependencies = (deps: Dependency[]) => deps.filter((d) => d.id !== TEMPEST_ID);

// The requirements of `requirements` that `tempestVersion` does not satisfy.
export const unmetTempest = (requirements: string[], tempestVersion: string) =>
	requirements.filter((r) => !satisfies(tempestVersion, r));
