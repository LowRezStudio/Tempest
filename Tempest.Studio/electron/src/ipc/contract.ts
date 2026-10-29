// Single source of truth for the renderer <-> host IPC surface, plus the
// JSON shapes `upk serve` returns (crates/tempest-assets/src/serve.rs and
// json.rs in Tempest.UnrealScript). TYPES ONLY: the renderer imports this
// file with `import type`, so nothing here reaches the bundle.
//
// Channel naming: `<domain>:<verb>` for invoke channels.

// ---- upk serve: shared value types ----

/** `{ ref, path, class }`: an object reference as `json::reference` renders it. */
export type ObjRef = { ref: number; path: string; class: string };

export type PropValue =
	| { t: "bool"; v: boolean }
	| { t: "byte"; v: number }
	| { t: "enum"; v: string }
	| { t: "int"; v: number }
	| { t: "float"; v: number }
	| { t: "name"; v: string }
	| { t: "str"; v: string }
	| { t: "object"; v: ObjRef }
	| { t: "delegate"; v: { object: ObjRef; function: string } }
	| { t: "struct"; v: Prop[] }
	| { t: "binary"; v: BinaryMember[] }
	/** `len`: the array's length when only its first elements were sent (`v`). */
	| { t: "array"; v: PropValue[]; len?: number }
	| { t: "raw"; v: { len: number; hex: string } };

export type Prop = {
	name: string;
	index: number;
	type: string;
	struct?: string;
	enum?: string;
	value: PropValue;
};

export type BinaryMember = { name: string; type: string; elems: PropValue[] };

export type ExportRow = {
	i: number;
	name: string;
	path: string;
	class: string;
	classRef: number;
	outer: number;
	super: number;
	archetype: number;
	size: number;
	offset: number;
	flags: string;
	flagsHex: string;
	exportFlags: number;
};

export type ImportRow = {
	i: number;
	name: string;
	path: string;
	class: string;
	classPackage: string;
	outer: number;
};

export type PackageSummary = {
	fileVersion: number;
	licenseeVersion: number;
	engineVersion: number;
	cookedContentVersion: number;
	packageFlags: number;
	packageFlagsText: string;
	compressionFlags: number;
	compressed: boolean;
	obscured: boolean;
	cooked: boolean;
	folder: string;
	guid: string;
	chunks: number;
	headerSize: number;
	names: number;
	imports: number;
	exports: number;
	additionalPackages: string[];
	textureAllocations: number;
};

export type PackageTables = {
	name: string;
	path: string;
	dirty: boolean;
	loadMs?: number;
	summary: PackageSummary;
	imports: ImportRow[];
	exports: ExportRow[];
};

/** The export table as the backend sends it: one column per field (the
 * renderer builds the `ExportRow`s, their paths and class names). */
export type ExportColumns = {
	names: string[];
	classes: number[];
	outers: number[];
	supers: number[];
	archetypes: number[];
	sizes: number[];
	offsets: number[];
	exportFlags: number[];
	/** Object flags as hex. */
	objectFlags: string[];
};
export type PackageTablesWire = Omit<PackageTables, "exports"> & { exports: ExportColumns };

export type WorkspacePackage = {
	name: string;
	path: string;
	size: number;
	script: boolean;
	map: boolean;
	open: boolean;
};

export type Workspace = {
	dir: string;
	scriptPackages: string[];
	textureIndex: boolean;
	loadMs: number;
	packages: WorkspacePackage[];
};

export type SearchHit = {
	package: string;
	path: string;
	index: number;
	object: string;
	class: string;
};

export type NativeInfo = { type: string; size: number; text: string };

export type ParsedObject = {
	index: number;
	path: string;
	class: string;
	row: ExportRow;
	dirty?: boolean;
} & (
	| { kind: "empty" }
	| { kind: "raw"; error: string; size: number; hex: string }
	| { kind: "script"; netIndex: number; text: string }
	| {
			kind: "object";
			netIndex: number;
			isClass: boolean;
			knownClass: boolean;
			stateFrame?: {
				node: ObjRef;
				state: ObjRef;
				probeMask: number;
				latentAction: number;
				stack: number;
				codeOffset: number | null;
			};
			component?: { templateOwnerClass: ObjRef; templateName: string | null };
			nativePrefix?: number;
			properties: Prop[];
			native: NativeInfo;
			hasRawValues: boolean;
	  }
);

export type MipLocation =
	| { kind: "inline" }
	| { kind: "unused" }
	| { kind: "missing" }
	| { kind: "cache"; file: string; offset: number; sizeOnDisk: number; size: number };

export type TextureInfo = {
	path: string;
	format: string;
	supported: boolean;
	width: number;
	height: number;
	lightmap: boolean;
	mips: {
		index: number;
		width: number;
		height: number;
		location: MipLocation;
		storedBytes: number;
	}[];
};

export type MeshInfo = {
	skeletal: boolean;
	lods: (
		| {
				index: number;
				vertices: number;
				triangles: number;
				bones: number;
				sections: { material: ObjRef; triangles: number }[];
		  }
		| { index: number; error: string }
	)[];
};

/** A notify of an AnimSequence (`Notifies[index]`). */
export type AnimNotify = {
	index: number;
	time: number;
	duration: number;
	notify: ObjRef;
	comment: string;
};

/** A skeletal mesh an animation can play on. */
export type AnimMesh = {
	path: string;
	index: number;
	object: string;
	matched: number;
	preview: boolean;
};

/** `anim.info`: what a sequence holds. */
export type AnimInfo = {
	/** The sequence described (the AnimSet's first one when `index` was the set). */
	index: number;
	numFrames: number;
	length: number;
	fps: number;
	rateScale: number;
	keyFormat: string;
	rotationFormat: string;
	translationFormat: string;
	tracks: number;
	keys: number;
	bytes: number;
	animSet: { index: number; path: string; preview: string | null };
	notifies: AnimNotify[];
	meshes: AnimMesh[];
	/** Every sequence of the AnimSet, by `SequenceName`. */
	sequences: { index: number; name: string }[];
};

export type AnimGlb = {
	mesh: { path: string; index: number; name: string };
	/** Track bones the mesh does not have (they do not move in the preview). */
	tracksWithoutBone: string[];
	warnings: string[];
	/** How many animations the file holds. */
	animations: number;
	glb?: Uint8Array;
	out?: string;
	bytes?: number;
};

export type MeshGlb = {
	/** GLB bytes (previews only; the host decodes the server's base64). */
	glb?: Uint8Array;
	bytes: number;
	vertices: number;
	triangles: number;
	materials: number;
	images: number;
	warnings: string[];
	out?: string;
};

export type Deps = {
	class: ObjRef;
	outer: ObjRef;
	archetype: ObjRef;
	super: ObjRef;
	references: ObjRef[];
	referrers: ObjRef[];
};

export type VerifyReport = {
	name: string;
	error: string | null;
	exports: number;
	full: number;
	raw: number;
	partialValues: number;
	withTail: number;
	mismatches: number;
	imageIdentical: boolean;
	mismatchSamples: string[];
	rawSamples: string[];
	ok: boolean;
};

export type MediaPreview =
	| {
			kind: "sound";
			ogg: Uint8Array;
			bytes: number;
			channels: number | null;
			sampleRate: number | null;
			duration: number | null;
	  }
	| { kind: "swf"; bytes: number; signature: string };

/** One Wwise media file (in a bank or streamed). */
export type WwiseMedia = {
	id: number;
	bytes: number;
	codec: string;
	channels: number;
	sampleRate: number;
	duration: number;
	streamed: boolean;
	bank: number | null;
	hash?: string;
	error?: string;
};

export type WwiseEvent = { id: number; name: string | null; media: number[] };

export type WwiseBank = {
	id: number;
	name: string | null;
	version?: number;
	objects?: number;
	error?: string;
	media: WwiseMedia[];
	events: WwiseEvent[];
};

/** A `.pck` file package. */
export type WwisePackage = {
	path: string;
	name: string;
	languages: string[];
	banks: WwiseBank[];
	streams: WwiseMedia[];
};

/** What an AkEvent / AkBank export resolves to. */
export type WwiseObject = {
	pck: string | null;
	bank: string;
	bankId: number;
	eventId: number | null;
	name: string;
	found?: boolean;
	media: WwiseMedia[];
	trail: { kind: string; id: number }[];
};

/** Edited PCM sent back to the backend (interleaved 16-bit, base64). */
export type PcmPayload = { pcm: string; channels: number; sampleRate: number; quality?: number };

/** A cooked particle system for the preview: every object as its parsed view. */
export type ParticleSystemJson = {
	root: number;
	system: ParsedObject;
	emitters: {
		index: number;
		path: string;
		class: string;
		props: ParsedObject;
		lods: {
			index: number;
			props: ParsedObject;
			required: ParsedObject | null;
			spawn: ParsedObject | null;
			typeData: ParsedObject | null;
			modules: ParsedObject[];
		}[];
	}[];
	materials: Record<
		string,
		{
			blendMode: string | null;
			twoSided: boolean | null;
			png: Uint8Array | null;
			texture: string | null;
			color: number[] | null;
		}
	>;
	meshes: Record<string, { path: string; index: number }>;
};

/** A decompiled class (the one owning the selected export). */
export type ScriptSource = {
	class: string;
	classIndex: number;
	source: string;
	/** The selected export's name when it is a member (function, state, …) of the class. */
	member: string | null;
	kind: string;
	classes: number;
};

/** Parsed from a `script.compile` rejection (`compile failed: {json}`). */
export type CompileFailure = { message: string; class?: string; line?: number; detail?: string };

export type ReplaceMatch = {
	index: number;
	object: string;
	/** Property path, or null for the object's own name. */
	prop: string | null;
	kind: "str" | "name" | "enum" | "object" | "objectname";
	current: string;
	replacement: string;
};

export type BatchEntry = {
	file: string;
	name: string;
	package: string;
	packagePath: string | null;
	object: string | null;
	index: number | null;
	kind: string | null;
	ok: boolean;
	message: string;
};

export type BatchResult = { entries: BatchEntry[]; ok: number; failed: number; ran: boolean };

export type MaterialPreview = {
	chain: { package: string; object: string; class: string }[];
	textures: {
		parameter: string;
		slot: "basecolor" | "normal" | "specular" | "emissive" | "mask" | "other";
		path: string;
		package: string;
		export: number;
		png: Uint8Array | null;
		error: string | null;
	}[];
	scalars: { name: string; value: number }[];
	vectors: { name: string; value: [number, number, number, number] }[];
	switches: { name: string; value: boolean }[];
};

type Target = { path: string; index: number };

/** Every `upk serve` method with its params and result. */
/** A column of a marshal row set: present in `count` of its rows. */
export type McsColumn = { name: string; token: number; kind: McsKind; count: number };
export type McsKind =
	| "unknown"
	| "byte"
	| "int"
	| "word"
	| "dword"
	| "float"
	| "double"
	| "qword"
	| "netid"
	| "datetime"
	| "string"
	| "rowset"
	| "guid"
	| "blob";
export type McsTable = { name: string; token: number; rows: number; columns: McsColumn[] };
export type McsHeaderField = { name: string; token: number; kind: McsKind; value: McsCell };
/** A scalar cell as JSON; 64-bit numbers travel as decimal strings; row sets as `{rows}`. */
export type McsCell = number | string | { rows: number } | null;
export type McsDoc = {
	id: string;
	path: string;
	function: string;
	flags: number;
	dirty: boolean;
	undo: string[];
	header: McsHeaderField[];
	tables: McsTable[];
};
export type McsSummary = { tokens: string; fields: number; docs: McsDoc[] };
/** A row set address: field names and row indices from the document root. */
export type McsPath = [string, number][];

/** A source of edited content for the mod cooker (`mod.changes`). */
export type CookSource = {
	id: string;
	/** Where the edited content is: the game file (unsaved or saved in place) or a copy. */
	source: string;
	/** The game file it changes. */
	target: string;
	rel: string;
	kind: "upk" | "mcts" | "pck";
	origin: "unsaved" | "saved" | "copy";
	script: boolean;
	doc?: string;
	/** Unix seconds: when the original was recorded (saved) or the copy written (copy). */
	when?: number;
	/** Unix seconds: when a file saved in place was last written. */
	modified?: number;
	/** What it is compared with: the Studio's record, the launcher's backup, or the game folder's file. */
	base: { kind: "recorded" | "backup" | "game"; path?: string; recorded?: number };
};
export type CookItem = {
	id: string;
	source: string;
	target: string;
	kind: "upk" | "mcts" | "pck";
	mode: "delta" | "whole";
};
export type CookPreview = {
	id?: string;
	rel?: string;
	mode?: "delta" | "whole" | "unchanged" | "extra";
	added?: number;
	changed?: number;
	deleted?: number;
	bytes?: number;
	changes?: { op: string; what: string }[];
	warnings?: string[];
	error?: string;
	/** Large objects of a delta kept as shared object files (one copy per mod). */
	shared?: { hash: string; bytes: number }[];
};

/** Linked: deltas against the game folder (small); portable: whole files (opens anywhere). */
export type TwrkMode = "linked" | "portable";
export type TwrkFile = {
	kind: "upk" | "mcts";
	path: string;
	rel: string | null;
	stored: "delta" | "full";
	entry: string;
	bytes: number;
	doc?: string;
	unchanged?: boolean;
};
export type McsRowsPage = {
	total: number;
	offset: number;
	rows: [number, Record<string, McsCell>][];
};
export type McsField = { name: string; token: number; kind: McsKind; value: McsCell };
export type McsStatus = { doc: string; dirty: boolean; undo: string[] };
export type McsMessage = { text: string; name: string; path: McsPath };
export type McsValidValue = { group: number; msgId: number; text: string };

export type SwfTag = {
	index: number;
	code: number;
	name: string;
	len: number;
	id: number | null;
	detail: string;
};
export type SwfImage = {
	tag: number;
	id: number;
	format: number;
	width: number;
	height: number;
	exportName: string;
	fileName: string;
};
export type SwfText = {
	tag: number;
	id: number;
	variable: string;
	text: string | null;
	fontId: number | null;
	fontClass: string | null;
	fontHeight: number | null;
	html: boolean;
	multiline: boolean;
};
/** Where a script lives: on the timeline `sprite` (0: the root) at stream index `tag`; `event`: the clip action of a placement; `button`: a button condition. */
export type SwfScriptLoc = { sprite?: number; tag: number; event?: number; button?: number };
export type SwfScript = {
	/** Short location key: `12`, `s40:3`, `s40:3e1` (clip event), `b7:0` (button). */
	key: string;
	loc: SwfScriptLoc;
	kind: "DoAction" | "DoInitAction" | "ClipAction" | "ButtonAction";
	/** Clip events or button conditions; a DoInitAction's sprite id. */
	label: string;
	/** Stream index of the tag in its timeline. */
	tag: number;
	/** The timeline the tag is on (0: the root). */
	owner: number;
	/** A DoInitAction's class sprite, else the sprite whose timeline holds it (null: the root). */
	sprite: number | null;
	/** Clip actions: the placement they belong to. */
	placed: { depth: number; character: number | null; name: string | null } | null;
	/** Frame (from 0) of its timeline. */
	frame: number;
	bytes: number;
	/** Root scripts: the strings of the top-level actions (the `string` edit). */
	strings: { index: number; text: string; action: string }[];
};
/** An imported movie (`ImportAssets2` URL) resolved to a workspace export. */
export type SwfLibrary = {
	url: string;
	path: string;
	object: string;
	index: number;
	bytes: number;
	swf: Uint8Array;
};
export type SwfInfo = {
	signature: string;
	version: number;
	compressed: boolean;
	width: number;
	height: number;
	frameRate: number;
	frames: number;
	tags: SwfTag[];
	images: SwfImage[];
	texts: SwfText[];
	scripts: SwfScript[];
	exports: { tag: number; id: number; name: string }[];
	imports: { tag: number; url: string; symbols: { id: number; name: string }[] }[];
};
/** A 2×3 transform: `x' = a·x + c·y + tx`, `y' = b·x + d·y + ty` (translation in pixels). */
export type SwfMatrix = { a: number; b: number; c: number; d: number; tx: number; ty: number };
/** Colour transform: `mul` factors (1 = unchanged) and `add` terms (-255..255), RGBA. */
export type SwfCxform = { mul: number[]; add: number[] };
export type SwfRect = { x: number; y: number; w: number; h: number };
/**
 * A pending movie edit. `text`/`string`/`image` address a top-level tag;
 * `place` rewrites the placement tag `tag` of the stream of `sprite` (0: the
 * root timeline; omitted fields stay, `null` removes a name or colour
 * transform) and `deletePlace` drops one.
 */
export type SwfEdit =
	| { kind: "text" | "string" | "image"; tag: number; index?: number; text: string }
	| {
			kind: "place";
			sprite: number;
			tag: number;
			matrix?: Partial<SwfMatrix>;
			name?: string | null;
			cxform?: SwfCxform | null;
			character?: number;
			depth?: number;
	  }
	| { kind: "deletePlace"; sprite: number; tag: number }
	/** A shape colour (`fill:g:i`, `stop:g:i:k`, `line:g:i`: `#rrggbbaa`) or line width (`width:g:i`: pixels), patched in place. */
	| { kind: "shapeColor"; tag: number; key: string; value: string }
	/** The shape redrawn from an SVG (fitted into its current bounds unless `fit` is false). */
	| { kind: "shapeSvg"; tag: number; svg: string; fit?: boolean }
	/** Edit-text properties, as `SwfEditTextProps` names them. */
	| { kind: "editText"; tag: number; props: Partial<SwfEditTextProps> }
	/** A static text record's characters and/or colour. */
	| { kind: "staticText"; tag: number; record: number; text?: string; color?: string }
	/** Characters added to a font from a TrueType/OpenType file. */
	| { kind: "fontGlyphs"; tag: number; file: string; chars: string }
	/** A script replaced by compiled ActionScript 2. */
	| { kind: "script"; loc: SwfScriptLoc; source: string }
	/** A script replaced by assembled P-code. */
	| { kind: "pcode"; loc: SwfScriptLoc; pcode: string };
export type SwfEditTextProps = {
	bounds: SwfRect;
	font: number | null;
	fontClass: string | null;
	/** Pixels. */
	height: number | null;
	/** `#rrggbbaa`. */
	color: string | null;
	maxLength: number | null;
	layout: boolean;
	align: "left" | "right" | "center" | "justify";
	leftMargin: number;
	rightMargin: number;
	indent: number;
	leading: number;
	variable: string;
	wordWrap: boolean;
	multiline: boolean;
	password: boolean;
	readOnly: boolean;
	autoSize: boolean;
	noSelect: boolean;
	border: boolean;
	wasStatic: boolean;
	html: boolean;
	useOutlines: boolean;
};
/** What the editor shows of one character (`swf.character`). */
export type SwfCharacterDetail =
	| {
			kind: "shape";
			tagName: string;
			/** Colours carry alpha (DefineShape3/4). */
			alpha: boolean;
			colors: { key: string; color: string }[];
			lines: { key: string; colorKey: string; width: number; color: string }[];
			bitmapFills: number;
			edges: number;
	  }
	| {
			kind: "font";
			tagName: string;
			name: string;
			glyphs: number;
			chars: string;
			bold: boolean;
			italic: boolean;
			layout: boolean;
			alignZones: boolean;
			editable: boolean;
	  }
	| {
			kind: "text";
			tagName: string;
			records: {
				text: string;
				font: number;
				fontName: string | null;
				height: number;
				color: string;
				x: number;
				y: number;
			}[];
	  }
	| {
			kind: "edittext";
			props: SwfEditTextProps;
			text: string | null;
			fonts: { id: number; name: string; imported: boolean }[];
	  }
	| {
			kind: "sound";
			format: string;
			rate: number;
			channels: number;
			samples: number | null;
			ext: "mp3" | "wav" | null;
	  }
	| { kind: "binary"; bytes: number }
	| {
			kind: Exclude<
				SwfCharacterKind,
				"shape" | "font" | "text" | "edittext" | "sound" | "binary"
			>;
			tagName?: string;
	  };
export type SwfCharacterKind =
	| "shape"
	| "morph"
	| "sprite"
	| "button"
	| "text"
	| "edittext"
	| "font"
	| "bitmap"
	| "image"
	| "subimage"
	| "sound"
	| "video"
	| "binary"
	| "import";
/** One entry of a movie's character dictionary (`swf.tree`). */
export type SwfCharacter = {
	id: number;
	kind: SwfCharacterKind;
	/** Top-level index of the defining tag; null for imports. */
	tag: number | null;
	tagName: string;
	/** Linkage names (`ExportAssets`), or the imported symbol name. */
	names: string[];
	/** Bounds in the character's own space (sprites at frame 1); null when unknown. */
	rect: SwfRect | null;
	frames: number;
	deps: number[];
	usedBy: number[];
	import: { url: string; name: string } | null;
	attached: { tag: number; name: string }[];
};
export type SwfTree = {
	characters: SwfCharacter[];
	stage: SwfRect;
	frames: number;
	rootFrames: number;
	labels: { frame: number; name: string }[];
};
/** One object of a display list (`swf.timeline`); tag indices are in the sprite's own stream. */
export type SwfEntry = {
	depth: number;
	character: number;
	name: string | null;
	className: string | null;
	matrix: SwfMatrix;
	cxform: SwfCxform | null;
	ratio: number | null;
	clipDepth: number | null;
	placedBy: number;
	matrixBy: number;
	changedBy: number;
	/** Tag code of the placing tag (4, 26, 70). */
	placeCode: number;
	since: number;
	/** The placement carries filters, a blend mode or other PlaceObject3 extras. */
	extras: boolean;
	/** Bounds in the sprite's space; null when the character's are unknown. */
	rect: SwfRect | null;
};
export type SwfTimeline = {
	sprite: number;
	/** Display list after each frame. */
	frames: SwfEntry[][];
	labels: { frame: number; name: string }[];
	actions: { frame: number; tag: number }[];
	tags: number;
};
export type SwfPreviewTarget =
	| { kind: "root"; frame: number }
	| { kind: "frame"; id: number; frame: number }
	| { kind: "character"; id: number; play?: boolean };

export interface UpkMethods {
	ping: { params: Record<string, never>; result: { version: string; pid: number } };
	"workspace.open": { params: { dir: string }; result: Workspace };
	"workspace.packages": { params: Record<string, never>; result: WorkspacePackage[] };
	"workspace.search": {
		params: { query: string; class?: string; limit?: number };
		result: { results: SearchHit[]; truncated: boolean };
	};
	"package.open": { params: { path: string }; result: PackageTables };
	"package.tables": { params: { path: string }; result: PackageTables };
	/** The name table of an open package. */
	"package.names": { params: { path: string }; result: string[] };
	/** One export row in full (path, flag names). */
	"package.export": { params: { path: string; index: number }; result: ExportRow };
	"package.close": { params: { path: string }; result: boolean };
	"package.dirty": { params: { path: string }; result: boolean };
	"package.save": {
		params: { path: string; out?: string };
		result: {
			out: string;
			reopened: boolean;
			dirty: boolean;
			undo: string[];
			dbReloaded?: boolean;
		};
	};
	"package.rewrite": {
		params: { path: string; out: string; compress: boolean; obscure?: boolean };
		result: { out: string; compress: boolean; obscure: boolean };
	};
	"package.verify": { params: { path: string }; result: VerifyReport };
	"package.undo": {
		params: { path: string };
		result: { label: string | null; remaining: string[]; dirty: boolean };
	};
	"package.undoList": { params: { path: string }; result: { labels: string[]; dirty: boolean } };
	"package.discard": { params: { path: string }; result: boolean };
	"object.get": { params: Target; result: ParsedObject };
	"object.text": { params: Target; result: string };
	"object.set": {
		params: Target & { prop: string; value: string };
		result: ParsedObject;
	};
	/** Add / duplicate / remove an array element, or reset a property to its class default. */
	"object.list": {
		params: Target & {
			prop: string;
			op: "add" | "duplicate" | "remove" | "removeProperty";
			at?: number;
		};
		result: ParsedObject;
	};
	"object.rename": { params: Target & { name: string }; result: ExportRow };
	"object.delete": { params: { path: string; indices: number[] }; result: { cleared: number } };
	"object.copy": {
		params: { src: string; index: number; dst: string; outer?: number | "top"; deep?: boolean };
		result: { index: number; path: string; warnings: string[] };
	};
	/** A copy of an export next to it (`…_copy`), same payload, no sub-objects. */
	"object.duplicate": { params: Target; result: { index: number; path: string } };
	"object.deps": { params: Target; result: Deps };
	"object.extract": { params: Target & { out: string }; result: { out: string; bytes: number } };
	"texture.info": { params: Target; result: TextureInfo };
	"texture.preview": {
		/** `maxSize`: the largest mip no wider or taller than that (thumbnails). */
		params: Target & { mip?: number; maxSize?: number };
		result: { png: Uint8Array; bytes: number };
	};
	"texture.export": {
		params: Target & { out: string; mip?: number };
		result: { out: string; bytes: number };
	};
	"texture.exportAll": {
		params: { path: string; outDir: string; png: boolean };
		result: { exported: number; failed: number; errors: string[]; outDir: string };
	};
	"texture.import": {
		/**
		 * `keepAlpha`: a translucent PNG going into a DXT1 texture is stored as DXT5.
		 * A PNG whose size is not a multiple of 4 going into a DXT texture (which
		 * D3D9 cannot create) is scaled to the nearest multiple of 4, or with
		 * `padBlocks` padded on the right and bottom like GFx's own images.
		 */
		params: Target & {
			file: string;
			mips?: boolean;
			format?: string;
			keepAlpha?: boolean;
			padBlocks?: boolean;
		};
		/** `source`: the file's own size. */
		result: TextureInfo & { source: { width: number; height: number } };
	};
	"mesh.info": { params: Target; result: MeshInfo };
	"anim.info": { params: Target; result: AnimInfo };
	"anim.preview": {
		params: Target & {
			mesh?: { path: string; index: number };
			textureSize?: number;
			textures?: boolean;
		};
		result: AnimGlb;
	};
	/** `out` ending in `.fbx` writes FBX, else GLB (mesh + skeleton + the animation). */
	/** `all` (or an AnimSet as `index`): every sequence of the AnimSet, named by `SequenceName`. */
	"anim.export": {
		params: Target & { out: string; mesh?: { path: string; index: number }; all?: boolean };
		result: AnimGlb;
	};
	"anim.files": { params: { file: string }; result: { animations: string[] } };
	/** Replace the motion from a glTF / FBX file (bones matched by name). */
	"anim.import": {
		params: Target & {
			file: string;
			mesh?: { path: string; index: number };
			animation?: number;
			fps?: number;
			/** Every animation of the file into the AnimSet's sequence of the same name (one undo step). */
			all?: boolean;
		};
		result: {
			numFrames: number;
			length: number;
			animation: string;
			bonesMatched: number;
			bonesMissing: string[];
			replaced: {
				index: number;
				sequence: string;
				animation: string;
				numFrames: number;
				length: number;
			}[];
			/** File animations no sequence is named like. */
			unmatched: string[];
			/** Sequences of the set the file has no animation for. */
			untouched: string[];
		};
	};
	"anim.edit": {
		params: Target &
			(
				| { op: "trim"; start: number; end: number }
				| { op: "retime"; length: number }
				| { op: "notifyMove"; notify: number; time: number; duration?: number }
				| { op: "notifyRemove"; notify: number }
				| { op: "notifyAdd"; notify: number; time: number }
			);
		result: AnimInfo;
	};
	"mesh.preview": {
		params: Target & { lod?: number; textures?: boolean; skin?: boolean; textureSize?: number };
		result: MeshGlb;
	};
	"mesh.export": {
		params: Target & { out: string; lod?: number; textures?: boolean; skin?: boolean };
		result: MeshGlb;
	};
	"mesh.import": {
		params: Target & { file: string; lod?: number };
		result: MeshInfo & {
			vertices: number;
			triangles: number;
			sections: number;
			chunks: number;
			warnings: string[];
		};
	};
	"object.replaceRaw": {
		params: Target & { file: string };
		result: { bytes: number; kind: string; error: string | null };
	};
	"media.preview": { params: Target; result: MediaPreview };
	"media.export": {
		params: Target & { out: string };
		result: { out: string; bytes: number; kind: string };
	};
	"media.import": {
		params: Target & { file: string; quality?: number };
		result: { kind: string; bytes: number };
	};
	"sound.replacePcm": {
		params: Target & PcmPayload;
		result: { bytes: number; duration: number; channels: number; sampleRate: number };
	};
	"wwise.packages": {
		params: Record<string, never>;
		result: { packages: { name: string; path: string; size: number }[] };
	};
	"wwise.open": { params: { path: string }; result: WwisePackage };
	"wwise.preview": {
		params: { path: string; id: number };
		result: WwiseMedia & { ogg: Uint8Array };
	};
	"wwise.export": {
		params: { path: string; id: number; out: string };
		result: { out: string; bytes: number };
	};
	/** Replaced media stay in memory: the `.pck` is unsaved until `package.save`. */
	"wwise.import": {
		params: { path: string; id: number; file: string; quality?: number };
		result: { path: string; bytes: number; dirty: boolean };
	};
	"wwise.replacePcm": {
		params: { path: string; id: number } & PcmPayload;
		result: { path: string; bytes: number; dirty: boolean };
	};
	"wwise.forObject": { params: Target; result: WwiseObject };
	/** Open assembly.dat and the Lang_*.dat files of a cooked folder (the workspace by default). */
	"mcts.open": { params: { dir?: string; reload?: boolean }; result: McsSummary };
	"mcts.summary": { params: Record<string, never>; result: McsSummary };
	"mcts.doc": { params: { doc: string }; result: McsDoc };
	"mcts.table": { params: { doc: string; path?: McsPath; table: string }; result: McsTable };
	"mcts.rows": {
		params: {
			doc: string;
			path?: McsPath;
			table: string;
			offset?: number;
			limit?: number;
			filter?: string;
			sort?: string;
			desc?: boolean;
		};
		result: McsRowsPage;
	};
	"mcts.row": { params: { doc: string; path: McsPath }; result: { fields: McsField[] } };
	"mcts.rowFull": { params: { doc: string; path: McsPath }; result: Record<string, unknown> };
	/** Set a field of a row from typed text (`text`) or JSON (`value`, null removes it). */
	"mcts.set": {
		params: { doc: string; path: McsPath; field: string; text?: string; value?: unknown };
		result: McsStatus & { row: Record<string, McsCell> };
	};
	"mcts.setHeader": {
		params: { doc: string; field: string; text?: string; value?: unknown };
		result: McsStatus;
	};
	"mcts.insert": {
		params: { doc: string; path?: McsPath; table: string; index?: number; clone?: number };
		result: McsStatus & { index: number };
	};
	/**
	 * Append a row at the end of a row set: empty (the last row's shape) or a copy of `clone`;
	 * the set's id column gets a fresh id, nested sets are renumbered and, with `messages`
	 * (default true), the copy's `*_MSG_ID`s get new messages with the same texts.
	 */
	"mcts.addRow": {
		params: {
			doc: string;
			path?: McsPath;
			table: string;
			clone?: number;
			/** A row as JSON (paste): used instead of `clone`. */
			row?: Record<string, unknown>;
			messages?: boolean;
		};
		result: McsStatus & {
			index: number;
			key: string | null;
			id: number | null;
			messages: { field: string; from: number; to: number }[];
		};
	};
	/** Rows that refer to an id (same id family: BOT_ID, REF_BOT_ID…), at most 500. */
	"mcts.usages": {
		params: { doc?: string; field: string; value: number };
		result: { usages: { path: McsPath; field: string; label: string }[]; truncated: boolean };
	};
	"mcts.delete": { params: { doc: string; path: McsPath }; result: McsStatus };
	"mcts.undo": { params: { doc: string }; result: McsStatus & { undone: string | null } };
	"mcts.save": { params: { doc: string; out?: string }; result: McsStatus & { out: string } };
	"mcts.discard": { params: { doc: string }; result: McsStatus };
	"mcts.find": {
		params: { doc: string; table: string; key: string; value: number | string };
		result: number[];
	};
	"mcts.messages": {
		params: { ids: number[]; langs?: string[] };
		result: Record<string, Record<string, McsMessage>>;
	};
	"mcts.setMessage": {
		params: { lang: string; id: number; text: string; name?: string };
		result: McsStatus;
	};
	"mcts.validValues": { params: Record<string, never>; result: Record<string, McsValidValue> };
	/** The next free ids of every kind (items and devices share one space). */
	"mcts.nextIds": {
		params: Record<string, never>;
		result: {
			item: number;
			device: number;
			bot: number;
			res: number;
			icon: number;
			fx: number;
			msg: number;
			deviceMode: number;
			skin: number;
			/** The id range in force (`mcts.idRange`), when one is set. */
			range: { start: number; end: number } | null;
		};
	};
	/** Clone a device/card (DEVICE_ID), item (ITEM_ID) or champion (BOT_ID) with fresh ids and new messages. */
	"mcts.create": {
		params: {
			kind: "device" | "card" | "item" | "champion";
			template: number;
			name: string;
			description?: string;
			shortDescription?: string;
			bot?: number;
			slot?: number;
			itemType?: number;
		};
		result: {
			kind: string;
			deviceId?: number;
			itemId?: number;
			botId?: number;
			messages: number[];
			device?: McsPath;
			item?: McsPath | null;
			bot?: McsPath;
			skin?: McsPath | null;
			devices?: unknown[];
			items?: unknown[];
		};
	};
	/** The Champion Vendor loot table (475, what the client lists as champions) against the champion bots. */
	"mcts.vendor": {
		params: Record<string, never>;
		result: {
			table: number;
			/** Row index of the loot table in DATA_SET_LOOT_TABLES, null when the table is absent. */
			index: number | null;
			total: number;
			active: number;
			listed: { bot: number | null; item: number; active: boolean }[];
			missing: { bot: number; item: number; reference: string; name: string }[];
		};
	};
	/** Add a champion's Character Class item to the Champion Vendor. */
	"mcts.vendorAdd": {
		params: { bot: number };
		result: { bot: number; item: number; vendor: McsPath | null; existed: boolean };
	};
	/** The GET_PROFILE_CLASSES row and inventory items of a champion, for the private server. */
	"mcts.serverRows": {
		params: { bot: number };
		result: { profileClass: Record<string, unknown>; inventory: unknown[] };
	};
	/** Write a mod folder: manifest, changed .dat files, paserver.json, Lua + UnrealScript scaffold. */
	"mcts.scaffold": {
		params: {
			dir: string;
			id: string;
			name?: string;
			version?: string;
			author?: string;
			bot?: number;
			devices?: number[];
			allLanguages?: boolean;
			package?: string;
			/** Ship the .dat files whose base is recorded as row deltas (default true). */
			deltas?: boolean;
		};
		result: {
			dir: string;
			files: string[];
			deltas: { file: string; ops: number; bytes: number }[];
			classes: string[];
			package: string;
		};
	};
	/** New ids come from `start..end` (a per-mod range so two Studio-made mods never collide); both absent clears it. */
	"mcts.idRange": {
		params: { start?: number; end?: number };
		result: UpkMethods["mcts.nextIds"]["result"];
	};
	"mcts.close": { params: Record<string, never>; result: boolean };
	/** Tags, texts, scripts and external images of a Scaleform movie export. */
	"swf.info": { params: { path: string; index: number; edits?: SwfEdit[] }; result: SwfInfo };
	/** The character dictionary of a movie (pending `edits` applied). */
	"swf.tree": { params: { path: string; index: number; edits?: SwfEdit[] }; result: SwfTree };
	/** The display list of a sprite (0: the root) after every frame. */
	"swf.timeline": {
		params: { path: string; index: number; sprite: number; edits?: SwfEdit[]; limit?: number };
		result: SwfTimeline;
	};
	/** A movie showing one character or timeline frame, cut to its bounds (`rect`). */
	"swf.preview": {
		params: {
			path: string;
			index: number;
			target: SwfPreviewTarget;
			edits?: SwfEdit[];
			/** Depths left out of a timeline target (drawn on their own layer). */
			exclude?: number[];
			/** Fixed stage area in the target's coordinates (pixels). */
			rect?: SwfRect;
		};
		result: {
			swf: Uint8Array;
			bytes: number;
			rect: SwfRect;
			images: number;
			missing: string[];
			/** Import tags left in the movie: 0 means its first picture is complete. */
			imports: number;
			/**
			 * Layouts: the cuts of the shared libraries the movie imports (at the URLs
			 * its imports now name). Absent for a playing target, which needs the
			 * whole libraries (`swf.libraries`).
			 */
			libraries?: { url: string; swf: Uint8Array }[];
		};
	};
	/** The movie bytes themselves (Scaleform GFx), for the Ruffle player. */
	"swf.bytes": {
		params: { path: string; index: number };
		result: { swf: Uint8Array; bytes: number };
	};
	/** The movie with its GFx external images embedded as standard bitmaps. */
	"swf.player": {
		params: { path: string; index: number; edits?: SwfEdit[] };
		result: { swf: Uint8Array; bytes: number; images: number; missing: string[] };
	};
	/** The workspace movies a movie imports (shared libraries, fonts), with their bytes. */
	"swf.libraries": {
		params: { path: string; index: number };
		/** `missing`: ImportAssets2 URLs no workspace package provides. */
		result: { libraries: SwfLibrary[]; missing: string[] };
	};
	/**
	 * One script (with the pending `edits` applied) as P-code and as decompiled
	 * ActionScript 2; `problems`: functions the decompiler kept as P-code.
	 */
	"swf.script": {
		params: {
			path: string;
			index: number;
			loc?: SwfScriptLoc;
			tag?: number;
			edits?: SwfEdit[];
		};
		result: { listing: string; pcode: string; source: string; problems: string[] };
	};
	/** Compile ActionScript (`source`) or assemble `pcode` without writing it; errors start with `line N:`. */
	"swf.compile": {
		params: { source?: string; pcode?: string };
		result: { bytes: number; pcode: string };
	};
	/**
	 * A new external image in a movie (pending `edits` written first): a Texture2D
	 * made from a PNG/DDS `file` with the settings of one the movie uses, its
	 * `DefineExternalImage2`, a shape drawing it (`shape`, default true) and,
	 * with `place`, that shape on a timeline frame. One undo step.
	 */
	"swf.addImage": {
		params: {
			path: string;
			index: number;
			file: string;
			/** Texture name (default `<movie>_<file name>`; made unique). */
			name?: string;
			shape?: boolean;
			/** Top-left corner in the timeline's pixels; `name` is the instance name. */
			place?: { sprite: number; frame: number; x: number; y: number; name?: string };
			format?: string;
			mips?: boolean;
			edits?: SwfEdit[];
		};
		result: {
			texture: { index: number; path: string; name: string };
			fileName: string;
			image: number;
			shape: number | null;
			width: number;
			height: number;
			placed: { sprite: number; frame: number; depth: number } | null;
			bytes: number;
			undo: string[];
		};
	};
	/** One character's details for the properties pane (pending `edits` applied). */
	"swf.character": {
		params: { path: string; index: number; id: number; edits?: SwfEdit[] };
		result: SwfCharacterDetail;
	};
	/**
	 * Write a file: `svg` of a `target` (character, sprite frame, root frame),
	 * `ttf` of font `id`, `sound` (MP3/WAV) or `binary` data of character `id`.
	 */
	"swf.export": {
		params: {
			path: string;
			index: number;
			format: "svg" | "ttf" | "sound" | "binary";
			out: string;
			id?: number;
			target?: SwfPreviewTarget;
			edits?: SwfEdit[];
		};
		result: { out: string; bytes: number; warnings: string[] };
	};
	/** Write edits (edit-text initial text, script strings, external image names, placements) back into the package. */
	"swf.apply": {
		params: { path: string; index: number; edits: SwfEdit[] };
		result: { edits: number; bytes: number; undo: string[] };
	};
	"particle.system": { params: Target & { textureSize?: number }; result: ParticleSystemJson };
	"particle.setTable": {
		params: Target & {
			prop: string;
			table: number[];
			timeScale?: number;
			startTime?: number;
			op?: number;
			numElements?: number;
			chunkSize?: number;
		};
		result: ParsedObject;
	};
	"script.source": { params: Target; result: ScriptSource };
	"script.compile": {
		params: { path: string; class: string; source: string };
		result: PackageTables & { warnings: string[]; compileMs: number };
	};
	"script.export": {
		params: { path: string; outDir: string };
		result: { dir: string; classes: number };
	};
	"material.preview": { params: Target & { textureSize?: number }; result: MaterialPreview };
	"material.setParameter": {
		params: Target & {
			name: string;
			scalar?: number;
			vector?: [number, number, number, number];
			textureSize?: number;
		};
		result: MaterialPreview & { changed: number };
	};
	"package.findReplace": {
		params: {
			path: string;
			query: string;
			replacement?: string;
			names?: boolean;
			apply?: boolean;
		};
		result: {
			matches: ReplaceMatch[];
			applied: number;
			errors: { index: number; object: string; prop: string | null; error: string }[];
			dirty: boolean;
		};
	};
	/** Write the session's unsaved packages and marshal files to a workspace file. */
	"twrk.save": {
		params: { out: string; mode: TwrkMode; state: unknown; include?: string[] };
		result: {
			out: string;
			mode: TwrkMode;
			files: TwrkFile[];
			size: number;
			warnings: string[];
		};
	};
	/** A workspace file's manifest, without applying it. */
	"twrk.read": {
		params: { path: string };
		result: {
			format: number;
			mode: TwrkMode;
			gameDir: string;
			saved: number;
			files: TwrkFile[];
			state: unknown;
		};
	};
	/** Put a workspace file's changes into the open workspace (all unsaved afterwards). */
	"twrk.apply": {
		params: { path: string };
		result: {
			packages: { path: string; stored: "delta" | "full"; changes: number; dirty: boolean }[];
			docs: string[];
			/** Wwise `.pck` files whose replaced media were put back (unsaved). */
			pcks?: string[];
			extras: string[];
			problems: string[];
			state: unknown;
			mode: TwrkMode;
		};
	};
	/** Every source of edited content a mod could take: unsaved edits, files saved in place, exported copies. */
	"mod.changes": { params: Record<string, never>; result: { files: CookSource[] } };
	/** What each cooker entry would put in the mod (nothing written). */
	"mod.preview": { params: { items: CookItem[] }; result: { items: CookPreview[] } };
	/** The game file a package or .dat from anywhere is a modified copy of (same name). */
	"mod.match": { params: { path: string }; result: { target: string | null } };
	/** Remember a file as a modified copy of a game file (listed by the cooker). */
	"mod.addCopy": { params: { path: string; target: string }; result: boolean };
	/** Drop an exported copy from the cooker's list (the file stays). */
	"mod.forgetCopy": { params: { path: string }; result: boolean };
	/** Write a launcher mod folder (manifest, README, icon, deltas/files, extra files). */
	"mod.cook": {
		params: {
			dir: string;
			id: string;
			name: string;
			version: string;
			authors: { name: string; link: string }[];
			readme?: string;
			dependencies?: string[];
			icon?: string;
			items: CookItem[];
			extras: { src: string; dest: string }[];
			/** "small" (LZMA, default) or "fast" (zstd: much faster, about 30 % bigger). */
			compression?: "small" | "fast";
		};
		result: {
			dir: string;
			entries: (CookPreview & { entry?: string })[];
			/** Shared object files (`shared/<hash>.tobj`): written once, used `uses` times by the deltas; `saved` bytes not repeated. */
			shared?: { files: number; uses: number; bytes: number; saved: number };
		};
	};
	"batch.plan": { params: { dir: string; recursive?: boolean }; result: BatchResult };
	"batch.run": {
		/**
		 * Imports into the open packages as unsaved edits (one undo step each).
		 * `packages`: the package files (paths from the plan) to change; every touched one when absent.
		 */
		params: { dir: string; recursive?: boolean; packages?: string[] };
		result: BatchResult;
	};
	"mod.dir": {
		params: {
			dir: string;
			id: string;
			name?: string;
			version?: string;
			author?: string;
			files: string[];
			/** Ship files whose base is recorded as deltas (default true); false copies every file whole. */
			deltas?: boolean;
		};
		result: { dir: string; files: string[]; entries: ModDirEntry[] };
	};
	/** Is the file's base (its state before the first in-place save) recorded, so a delta can be made? */
	"delta.status": { params: { path: string }; result: DeltaStatus };
	/** Take the file as it is now as the base (replacing any earlier record). */
	"delta.record": {
		/** `from`: another file holding the original (a backup, a clean install). */
		params: { path: string; from?: string };
		result: DeltaStatus & { ms: number };
	};
	"delta.reset": { params: { path: string }; result: boolean };
	"delta.info": {
		params: { file: string };
		result: { kind: string; target: string; header: Record<string, unknown> };
	};
	/** One delta file from a game file and its recorded base. */
	"delta.create": {
		params: { path: string; out: string; target?: string };
		result: {
			out: string;
			target: string;
			kind: string;
			ops: { op: string; path: string; class: string }[];
			tables: unknown;
			bytes: number;
			warnings: string[];
		};
	};
}

/** How one file went into a mod folder (`mod.dir`). */
export interface ModDirEntry {
	/** Game-relative path (`ChaosGame/CookedPCConsole/Foo.upk`). */
	file: string;
	/** `delta`: only what changed; `copy`: the whole file (no base recorded); `unchanged`: skipped. */
	mode: "delta" | "copy" | "unchanged";
	ops: number;
	bytes: number;
	warnings: string[];
}

export interface DeltaStatus {
	recorded: boolean;
	/** Unix seconds when the base was recorded. */
	when?: number;
	size?: number;
	objects?: number;
	target?: string;
}

export type UpkMethod = keyof UpkMethods;

/** Status of the backend process. */
export type McpInfo = { url: string; token: string; configPath: string; running: boolean };

export type UpkStatus = {
	running: boolean;
	binary: string | null;
	pid: number | null;
	error: string | null;
};

/** An event line pushed by `upk serve` (no request id). */
export type UpkEvent = { event: string; [key: string]: unknown };

/** A framed response as the host hands it to the renderer: the result JSON
 * bytes (or an already parsed `value` for unframed replies) and the binary
 * payloads referenced by `{"$bin": n}` markers inside it. */
export type UpkFrame = { json: Uint8Array | null; value?: unknown; bins: Uint8Array[] };

// ---- Dialogs ----

export type DialogFilter = { name: string; extensions: string[] };
export type OpenDialogOptions = {
	directory?: boolean;
	multiple?: boolean;
	filters?: DialogFilter[];
	title?: string;
	defaultPath?: string;
};
export type SaveDialogOptions = {
	defaultPath?: string;
	filters?: DialogFilter[];
	title?: string;
};

/** OS info injected by preload (see `window.__os`). */
/** What a new window opens once its renderer is up (pulled through `window:startup`). */
export type WindowStartup = { dir?: string; files?: string[] };

export type OsInfo = {
	platform: "windows" | "macos" | "linux";
	arch: string;
};

/**
 * The typed invoke surface. `args: void` channels take no payload.
 * Handlers may reject; the renderer sees the rejection as an `Error`.
 */
export interface IpcContract {
	"app:version": { args: void; result: string };
	"app:home": { args: void; result: string };
	/** The user's Documents folder (default home of mod projects). */
	"app:documents": { args: void; result: string };
	"app:temp-path": { args: { name: string }; result: string };

	"window:set-title": { args: { title: string }; result: void };
	/** Open another Studio window (with its own backend), optionally on a workspace or packages. */
	"window:new": { args: WindowStartup; result: void };
	/** What this window was asked to open when it was created; `done` clears it once applied. */
	"window:startup": { args: { done?: boolean }; result: WindowStartup | null };

	/**
	 * The crash-recovery workspace file of a game folder (`<userData>/recovery/<hash>.twrk`):
	 * its path and, when it exists, its modification time; `clear` deletes it.
	 */
	"workspace:recovery": {
		args: { dir: string; clear?: boolean };
		result: { path: string; exists: boolean; mtime: number | null };
	};

	/** tempest-cli for the mod cooker: where it is and whether a signing key exists. */
	"cli:status": {
		args: Record<string, never>;
		result: { path: string | null; available: boolean; signingKey: string; hasKey: boolean };
	};
	/** Run `tempest-cli mod <args>` (pack, sign, verify, install, publish, login, gen-key…). */
	"cli:mod": {
		args: { args: string[] };
		result: { code: number; stdout: string; stderr: string };
	};
	/** A fresh staging folder path for a cooked mod. */
	"cli:staging": { args: { id: string }; result: string };
	/** Zoom the window's content (UI scale), 0.6–2. */
	"window:zoom": { args: { factor: number }; result: void };

	"dialog:open": { args: OpenDialogOptions; result: string[] | null };
	"dialog:save": { args: SaveDialogOptions; result: string | null };

	"fs:exists": { args: { path: string }; result: boolean };
	"fs:read-text": { args: { path: string }; result: string };
	"fs:write-text": { args: { path: string; text: string }; result: void };
	"fs:write-bytes": { args: { path: string; bytes: Uint8Array }; result: void };
	"fs:remove": { args: { path: string }; result: void };
	/** Create a folder (and its parents). */
	"fs:mkdir": { args: { path: string }; result: void };
	/** Files of a folder, recursively, relative to it (for a project's mod-files/). */
	"fs:list": { args: { path: string }; result: string[] };

	"shell:show-item": { args: { path: string }; result: void };
	"shell:open-path": { args: { path: string }; result: string };

	/** One `upk serve` call. Rejects with the server's error text. */
	"upk:call": { args: { method: UpkMethod; params: unknown }; result: unknown };
	/** Where the MCP server listens and the token clients must send. */
	"mcp:info": { args: void; result: McpInfo };
	"upk:status": { args: void; result: UpkStatus };
	"upk:restart": { args: void; result: UpkStatus };
}

export type IpcChannel = Extract<keyof IpcContract, string>;

/** Events the host pushes to the renderer. */
export interface IpcEvents {
	/** A progress or notification line from `upk serve`. */
	"upk:event": UpkEvent;
	/** The backend died (crashed or was restarted); open packages are gone. */
	"upk:exit": { code: number | null; signal: string | null };
	/** A .upk file was opened through the OS (file association, argv). */
	"open-files": string[];
}

export type IpcEventChannel = Extract<keyof IpcEvents, string>;

/** Shape of the preload bridge (`window.electronAPI`). */
export type PreloadApi = {
	invoke: <K extends IpcChannel>(
		channel: K,
		...args: IpcContract[K]["args"] extends void ? [] : [IpcContract[K]["args"]]
	) => Promise<IpcContract[K]["result"]>;
	on: <K extends IpcEventChannel>(
		channel: K,
		callback: (payload: IpcEvents[K]) => void,
	) => () => void;
	getPathForFile: (file: File) => string;
};
