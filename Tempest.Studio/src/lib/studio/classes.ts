// What the inspector can do with an export, decided from its class name
// (content packages lowercase everything, so compare case-insensitively).
export type Preview = "texture" | "mesh" | "sound" | "swf" | "material" | "none";

const TEXTURES = new Set([
	"texture2d",
	"lightmaptexture2d",
	"shadowmaptexture2d",
	"terrainweightmaptexture",
	"textureflipbook",
]);
const MESHES = new Set(["staticmesh", "skeletalmesh", "fracturedstaticmesh", "speedtree"]);

export function previewFor(className: string, nativeType?: string): Preview {
	const c = className.toLowerCase();
	if (nativeType === "Texture2D" || TEXTURES.has(c)) return "texture";
	if (nativeType === "StaticMesh" || nativeType === "SkeletalMesh" || MESHES.has(c)) {
		return "mesh";
	}
	if (c === "soundnodewave") return "sound";
	if (c === "swfmovie" || c === "gfxmovieinfo") return "swf";
	if (c.startsWith("materialinstance") || c === "material" || c === "decalmaterial") {
		return "material";
	}
	return "none";
}

/** Lucide-ish category for the tree icon. */
export type Category =
	| "package"
	| "texture"
	| "material"
	| "mesh"
	| "sound"
	| "swf"
	| "anim"
	| "level"
	| "actor"
	| "script"
	| "physics"
	| "font"
	| "other";

export function categoryFor(className: string): Category {
	const c = className.toLowerCase();
	if (c === "package" || c === "") return "package";
	if (c.includes("texture")) return "texture";
	if (c.startsWith("material") || c.startsWith("decalmaterial")) return "material";
	if (c.includes("mesh") || c === "speedtree") return "mesh";
	if (c.startsWith("sound") || c.startsWith("ak")) return "sound";
	if (c === "swfmovie" || c === "gfxmovieinfo") return "swf";
	if (c.startsWith("anim") || c.includes("morphtarget") || c.includes("facefx")) return "anim";
	if (
		c === "level" ||
		c === "world" ||
		c === "model" ||
		c === "polys" ||
		c.startsWith("worldinfo")
	) {
		return "level";
	}
	if (
		c === "class" ||
		c === "function" ||
		c === "state" ||
		c === "scriptstruct" ||
		c === "enum" ||
		c === "const" ||
		c.endsWith("property")
	) {
		return "script";
	}
	if (c.startsWith("rb_") || c.startsWith("physics") || c.startsWith("apex")) return "physics";
	if (c === "font" || c === "multifont") return "font";
	if (
		c.endsWith("actor") ||
		c.endsWith("component") ||
		c.startsWith("seq") ||
		c.startsWith("interp") ||
		c.startsWith("kismet")
	) {
		return "actor";
	}
	return "other";
}

const SCRIPT_CLASSES = new Set([
	"class",
	"function",
	"state",
	"scriptstruct",
	"enum",
	"const",
	"textbuffer",
]);

/** Exports the UnrealScript toolchain owns: classes and their members. */
export function isScriptExport(row: {
	class: string;
	classRef: number;
	size: number;
	name: string;
}): boolean {
	if (row.classRef === 0 && row.size > 0) return true;
	const c = row.class.toLowerCase();
	// Class default objects are the class's defaultproperties block.
	if (row.name.toLowerCase().startsWith("default__")) return true;
	return SCRIPT_CLASSES.has(c) || c.endsWith("property");
}

export const CONTAINS_SCRIPT = 0x0020_0000;
