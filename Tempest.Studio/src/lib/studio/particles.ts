/**
 * Cooked particle systems: the model decoded from `particle.system` (baked
 * distribution tables, module properties) and a CPU simulation of the common
 * sprite / mesh emitter modules, for the preview.
 */
import type { ParsedObject, Prop, PropValue } from "../../../electron/src/ipc/contract";

/** One `FRawDistribution*`: `[min, max]` then entries of `chunk` floats. */
export interface Dist {
	/** Property name on the module (as the package spells it). */
	prop: string;
	kind: "float" | "vector";
	/** 0 none, 1 random (two values per entry), 2 extreme. */
	op: number;
	numElements: number;
	chunk: number;
	/** Components per value (1 or 3). */
	comps: number;
	timeScale: number;
	startTime: number;
	/** Entries, each `chunk` floats. */
	entries: number[][];
	/** The two header floats. */
	min: number;
	max: number;
}

function num(v: PropValue | undefined, d = 0): number {
	if (!v) return d;
	if (v.t === "float" || v.t === "int" || v.t === "byte") return v.v;
	if (v.t === "bool") return v.v ? 1 : 0;
	return d;
}

export function propOf(obj: ParsedObject | null | undefined, name: string): Prop | undefined {
	if (!obj || obj.kind !== "object") return undefined;
	const n = name.toLowerCase();
	return obj.properties.find((p) => p.name.toLowerCase() === n && p.index === 0);
}

export function numProp(obj: ParsedObject | null | undefined, name: string, d = 0): number {
	return num(propOf(obj, name)?.value, d);
}

export function boolProp(obj: ParsedObject | null | undefined, name: string, d: boolean): boolean {
	const v = propOf(obj, name)?.value;
	return v?.t === "bool" ? v.v : d;
}

export function enumProp(obj: ParsedObject | null | undefined, name: string, d = ""): string {
	const v = propOf(obj, name)?.value;
	return v?.t === "enum" || v?.t === "name" ? v.v.toLowerCase() : d;
}

export function refPath(obj: ParsedObject | null | undefined, name: string): string | null {
	const v = propOf(obj, name)?.value;
	return v?.t === "object" && v.v.ref !== 0 ? v.v.path : null;
}

function member(members: Prop[], name: string): PropValue | undefined {
	const n = name.toLowerCase();
	return members.find((m) => m.name.toLowerCase() === n)?.value;
}

/** Decode a raw distribution property; `null` when it is not one. */
export function distOf(obj: ParsedObject | null | undefined, name: string): Dist | null {
	const p = propOf(obj, name);
	if (!p || p.value.t !== "struct") return null;
	const kind = (p.struct ?? "").toLowerCase().includes("vector") ? "vector" : "float";
	return decodeDist(p.name, kind, p.value.v);
}

export function decodeDist(prop: string, kind: "float" | "vector", members: Prop[]): Dist {
	const op = num(member(members, "Op"), 0);
	const comps0 = kind === "vector" ? 3 : 1;
	const numElements = num(member(members, "LookupTableNumElements"), op === 0 ? 1 : 2) || 1;
	const chunk =
		num(member(members, "LookupTableChunkSize"), comps0 * numElements) || comps0 * numElements;
	const comps = Math.max(1, Math.round(chunk / numElements));
	const tv = member(members, "LookupTable");
	const table: number[] = tv?.t === "array" ? tv.v.map((x) => num(x)) : [];
	const entries: number[][] = [];
	for (let i = 2; i + chunk <= table.length; i += chunk) entries.push(table.slice(i, i + chunk));
	return {
		prop,
		kind,
		op,
		numElements,
		chunk,
		comps,
		timeScale: num(member(members, "LookupTableTimeScale"), 0),
		startTime: num(member(members, "LookupTableStartTime"), 0),
		entries,
		min: table[0] ?? 0,
		max: table[1] ?? 0,
	};
}

/** The flat table (`[min, max, ...entries]`) of a distribution. */
export function distTable(d: Dist): number[] {
	const flat = d.entries.flat();
	let min = Number.POSITIVE_INFINITY;
	let max = Number.NEGATIVE_INFINITY;
	for (const v of flat) {
		min = Math.min(min, v);
		max = Math.max(max, v);
	}
	if (flat.length === 0) {
		min = 0;
		max = 0;
	}
	return [min, max, ...flat];
}

/** Does this distribution vary over time (more than one distinct entry)? */
export function isCurve(d: Dist): boolean {
	if (d.entries.length < 2) return false;
	const first = d.entries[0];
	return d.entries.some((e) => e.some((v, i) => v !== first[i]));
}

/**
 * `FRawDistribution::GetValue`: the entry at `time`, lerped, one value per
 * component; `rnd` picks between the two values of a random/extreme entry.
 */
export function sampleDist(d: Dist, time: number, rnd: number[] | null, out: number[]): number[] {
	const n = d.entries.length;
	for (let c = 0; c < d.comps; c++) out[c] = 0;
	if (n === 0) return out;
	let t = (time - d.startTime) * d.timeScale;
	if (!Number.isFinite(t)) t = 0;
	t = Math.max(0, Math.min(n - 1, t));
	const i = Math.floor(t);
	const a = t - i;
	const e1 = d.entries[i];
	const e2 = d.entries[Math.min(n - 1, i + 1)];
	for (let c = 0; c < d.comps; c++) {
		const v1 = e1[c] + (e2[c] - e1[c]) * a;
		if (d.numElements >= 2) {
			const k = d.comps + c;
			const w1 = e1[k] + (e2[k] - e1[k]) * a;
			const r = rnd ? rnd[c] : 0.5;
			out[c] = d.op === 2 ? (r < 0.5 ? v1 : w1) : v1 + (w1 - v1) * r;
		} else {
			out[c] = v1;
		}
	}
	return out;
}

// ----------------------------------------------------------------------------
// System model
// ----------------------------------------------------------------------------

export interface LodJson {
	index: number;
	props: ParsedObject;
	required: ParsedObject | null;
	spawn: ParsedObject | null;
	typeData: ParsedObject | null;
	modules: ParsedObject[];
}

export interface EmitterJson {
	index: number;
	path: string;
	class: string;
	props: ParsedObject;
	lods: LodJson[];
}

export interface MaterialJson {
	blendMode: string | null;
	twoSided: boolean | null;
	png: Uint8Array | null;
	texture: string | null;
	color: number[] | null;
}

export interface SystemJson {
	root: number;
	system: ParsedObject;
	emitters: EmitterJson[];
	materials: Record<string, MaterialJson>;
	meshes: Record<string, { path: string; index: number }>;
}

export interface Burst {
	count: number;
	countLow: number;
	time: number;
}

/** What the simulation needs from one emitter's LOD 0. */
export interface EmitterModel {
	index: number;
	name: string;
	enabled: boolean;
	material: string | null;
	meshPath: string | null;
	/** Mesh emitters use the required module's material when set. */
	overrideMaterial: boolean;
	duration: number;
	loops: number;
	delay: number;
	subH: number;
	subV: number;
	/** "none" | "linear" | "random" */
	subMethod: string;
	localSpace: boolean;
	screenAlignment: string;
	axisLock: string;
	rate: Dist | null;
	rateScale: Dist | null;
	bursts: Burst[];
	lifetime: Dist | null;
	size: Dist | null;
	sizeMultiplyLife: Dist | null;
	sizeMultiplyMask: [boolean, boolean, boolean];
	sizeScale: Dist | null;
	velocity: Dist | null;
	velocityRadial: Dist | null;
	velocityOverLife: Dist | null;
	velocityOverLifeAbsolute: boolean;
	acceleration: Dist | null;
	color: Dist | null;
	alpha: Dist | null;
	colorOverLife: Dist | null;
	alphaOverLife: Dist | null;
	colorScaleOverLife: Dist | null;
	alphaScaleOverLife: Dist | null;
	rotation: Dist | null;
	rotationRate: Dist | null;
	subImageIndex: Dist | null;
	locations: Dist[];
	sphere: {
		radius: Dist | null;
		surface: boolean;
		velocity: boolean;
		velocityScale: Dist | null;
		location: Dist | null;
	} | null;
	cylinder: {
		radius: Dist | null;
		height: Dist | null;
		axis: string;
		surface: boolean;
		velocity: boolean;
		velocityScale: Dist | null;
		location: Dist | null;
	} | null;
	orbitOffset: Dist | null;
	orbitRotation: Dist | null;
	orbitRotationRate: Dist | null;
	meshRotation: Dist | null;
	meshRotationRate: Dist | null;
	/** Modules the simulation does not interpret. */
	unsupported: string[];
	peak: number;
}

const SUPPORTED = new Set([
	"particlemodulelifetime",
	"particlemodulesize",
	"particlemodulesizemultiplylife",
	"particlemodulesizescale",
	"particlemodulevelocity",
	"particlemodulevelocityoverlifetime",
	"particlemoduleacceleration",
	"particlemodulecolor",
	"particlemodulecoloroverlife",
	"particlemodulecolorscaleoverlife",
	"particlemodulerotation",
	"particlemodulerotationrate",
	"particlemodulesubuv",
	"particlemodulelocation",
	"particlemodulelocationprimitivesphere",
	"particlemodulelocationprimitivecylinder",
	"particlemoduleorbit",
	"particlemodulemeshrotation",
	"particlemodulemeshrotationrate",
	"particlemoduleorientationaxislock",
	"particlemodulecameraoffset",
	"particlemoduleparameterdynamic",
	"particlemodulevelocityinheritparent",
	"particlemodulespawn",
	"particlemodulerequired",
	"particlemoduletypedatamesh",
]);

export function buildEmitter(e: EmitterJson): EmitterModel {
	const lod = e.lods[0];
	const req = lod?.required ?? null;
	const spawn = lod?.spawn ?? null;
	const td = lod?.typeData ?? null;
	const m: EmitterModel = {
		index: e.index,
		name: (() => {
			const v = propOf(e.props, "EmitterName")?.value;
			return v?.t === "name" ? v.v : (e.path.split(".").pop() ?? e.path);
		})(),
		enabled:
			boolProp(lod?.props, "bEnabled", true) &&
			enumProp(e.props, "EmitterRenderMode", "erm_normal") !== "erm_none",
		material: refPath(req, "Material"),
		meshPath:
			td && td.class.toLowerCase() === "particlemoduletypedatamesh"
				? refPath(td, "Mesh")
				: null,
		overrideMaterial: boolProp(td, "bOverrideMaterial", false),
		duration: numProp(req, "EmitterDuration", 1),
		loops: numProp(req, "EmitterLoops", 0),
		delay: numProp(req, "EmitterDelay", 0),
		subH: Math.max(1, numProp(req, "SubImages_Horizontal", 1)),
		subV: Math.max(1, numProp(req, "SubImages_Vertical", 1)),
		subMethod: enumProp(req, "InterpolationMethod", "psuvim_none").replace("psuvim_", ""),
		localSpace: boolProp(req, "bUseLocalSpace", false),
		screenAlignment: enumProp(req, "ScreenAlignment", "psa_square"),
		axisLock: "",
		rate: distOf(spawn, "Rate"),
		rateScale: distOf(spawn, "RateScale"),
		bursts: [],
		lifetime: null,
		size: null,
		sizeMultiplyLife: null,
		sizeMultiplyMask: [true, true, true],
		sizeScale: null,
		velocity: null,
		velocityRadial: null,
		velocityOverLife: null,
		velocityOverLifeAbsolute: false,
		acceleration: null,
		color: null,
		alpha: null,
		colorOverLife: null,
		alphaOverLife: null,
		colorScaleOverLife: null,
		alphaScaleOverLife: null,
		rotation: null,
		rotationRate: null,
		subImageIndex: null,
		locations: [],
		sphere: null,
		cylinder: null,
		orbitOffset: null,
		orbitRotation: null,
		orbitRotationRate: null,
		meshRotation: null,
		meshRotationRate: null,
		unsupported: [],
		peak: numProp(lod?.props, "PeakActiveParticles", 64),
	};
	const bl = propOf(spawn, "BurstList")?.value;
	if (bl?.t === "array") {
		for (const b of bl.v) {
			if (b.t === "struct") {
				const count = num(member(b.v, "Count"), 0);
				m.bursts.push({
					count,
					countLow: num(member(b.v, "CountLow"), -1),
					time: num(member(b.v, "Time"), 0),
				});
			}
		}
	}
	if (td && !SUPPORTED.has(td.class.toLowerCase())) m.unsupported.push(td.class);
	for (const mod of lod?.modules ?? []) {
		const c = mod.class.toLowerCase();
		if (!boolProp(mod, "bEnabled", true)) continue;
		switch (c) {
			case "particlemodulelifetime": {
				m.lifetime = distOf(mod, "Lifetime");
				break;
			}
			case "particlemodulesize": {
				m.size = distOf(mod, "StartSize");
				break;
			}
			case "particlemodulesizemultiplylife": {
				m.sizeMultiplyLife = distOf(mod, "LifeMultiplier");
				m.sizeMultiplyMask = [
					boolProp(mod, "MultiplyX", true),
					boolProp(mod, "MultiplyY", true),
					boolProp(mod, "MultiplyZ", true),
				];
				break;
			}
			case "particlemodulesizescale": {
				m.sizeScale = distOf(mod, "SizeScale");
				break;
			}
			case "particlemodulevelocity": {
				m.velocity = distOf(mod, "StartVelocity");
				m.velocityRadial = distOf(mod, "StartVelocityRadial");
				break;
			}
			case "particlemodulevelocityoverlifetime": {
				m.velocityOverLife = distOf(mod, "VelOverLife");
				m.velocityOverLifeAbsolute = boolProp(mod, "Absolute", false);
				break;
			}
			case "particlemoduleacceleration": {
				m.acceleration = distOf(mod, "Acceleration");
				break;
			}
			case "particlemodulecolor": {
				m.color = distOf(mod, "StartColor");
				m.alpha = distOf(mod, "StartAlpha");
				break;
			}
			case "particlemodulecoloroverlife": {
				m.colorOverLife = distOf(mod, "ColorOverLife");
				m.alphaOverLife = distOf(mod, "AlphaOverLife");
				break;
			}
			case "particlemodulecolorscaleoverlife": {
				m.colorScaleOverLife = distOf(mod, "ColorScaleOverLife");
				m.alphaScaleOverLife = distOf(mod, "AlphaScaleOverLife");
				break;
			}
			case "particlemodulerotation": {
				m.rotation = distOf(mod, "StartRotation");
				break;
			}
			case "particlemodulerotationrate": {
				m.rotationRate = distOf(mod, "StartRotationRate");
				break;
			}
			case "particlemodulesubuv": {
				m.subImageIndex = distOf(mod, "SubImageIndex");
				break;
			}
			case "particlemodulelocation": {
				const d = distOf(mod, "StartLocation");
				if (d) m.locations.push(d);
				break;
			}
			case "particlemodulelocationprimitivesphere": {
				m.sphere = {
					radius: distOf(mod, "StartRadius"),
					surface: boolProp(mod, "SurfaceOnly", false),
					velocity: boolProp(mod, "Velocity", false),
					velocityScale: distOf(mod, "VelocityScale"),
					location: distOf(mod, "StartLocation"),
				};
				break;
			}
			case "particlemodulelocationprimitivecylinder": {
				m.cylinder = {
					radius: distOf(mod, "StartRadius"),
					height: distOf(mod, "StartHeight"),
					axis: enumProp(mod, "HeightAxis", "psphc_z"),
					surface: boolProp(mod, "SurfaceOnly", false),
					velocity: boolProp(mod, "Velocity", false),
					velocityScale: distOf(mod, "VelocityScale"),
					location: distOf(mod, "StartLocation"),
				};
				break;
			}
			case "particlemoduleorbit": {
				m.orbitOffset = distOf(mod, "OffsetAmount");
				m.orbitRotation = distOf(mod, "RotationAmount");
				m.orbitRotationRate = distOf(mod, "RotationRateAmount");
				break;
			}
			case "particlemodulemeshrotation": {
				m.meshRotation = distOf(mod, "StartRotation");
				break;
			}
			case "particlemodulemeshrotationrate": {
				m.meshRotationRate = distOf(mod, "StartRotationRate");
				break;
			}
			case "particlemoduleorientationaxislock": {
				m.axisLock = enumProp(mod, "LockAxisFlags", "");
				break;
			}
			default: {
				if (!SUPPORTED.has(c)) m.unsupported.push(mod.class);
			}
		}
	}
	return m;
}

// ----------------------------------------------------------------------------
// Simulation
// ----------------------------------------------------------------------------

export interface Particle {
	age: number;
	life: number;
	pos: [number, number, number];
	vel: [number, number, number];
	baseSize: [number, number, number];
	size: [number, number, number];
	rot: number;
	rotRate: number;
	baseColor: [number, number, number, number];
	color: [number, number, number, number];
	sub: number;
	subRnd: number;
	orbit: [number, number, number];
	orbitRot: [number, number, number];
	orbitRate: [number, number, number];
	meshRot: [number, number, number];
	meshRate: [number, number, number];
	accel: [number, number, number];
}

const TAU = Math.PI * 2;

export class EmitterSim {
	particles: Particle[] = [];
	time = 0;
	loopsDone = 0;
	finished = false;
	private spawnAccum = 0;
	private burstsDone = new Set<number>();
	private rnd: () => number;
	private scratch: number[] = [0, 0, 0];
	private scratch2: number[] = [0, 0, 0];

	constructor(
		public model: EmitterModel,
		seed = 1,
	) {
		this.rnd = mulberry(seed);
	}

	reset(seed = 1): void {
		this.particles = [];
		this.time = 0;
		this.loopsDone = 0;
		this.finished = false;
		this.spawnAccum = 0;
		this.burstsDone.clear();
		this.rnd = mulberry(seed);
	}

	get emitterTime(): number {
		return Math.max(0, this.time - this.model.delay);
	}

	step(dt: number): void {
		const m = this.model;
		this.time += dt;
		const et = this.emitterTime;
		const duration = Math.max(0.001, m.duration);
		const loop = Math.floor(et / duration);
		if (m.loops > 0 && loop >= m.loops) {
			this.finished = true;
		} else if (loop > this.loopsDone) {
			this.loopsDone = loop;
			this.burstsDone.clear();
		}
		const local = et - loop * duration;
		// Spawn.
		if (!this.finished && this.time >= m.delay && m.enabled) {
			let rate = m.rate ? sampleDist(m.rate, local, null, this.scratch)[0] : 0;
			if (m.rateScale) rate *= sampleDist(m.rateScale, local, null, this.scratch2)[0];
			this.spawnAccum += Math.max(0, rate) * dt;
			let count = Math.floor(this.spawnAccum);
			this.spawnAccum -= count;
			for (const [k, b] of m.bursts.entries()) {
				if (!this.burstsDone.has(k) && local >= b.time * duration) {
					this.burstsDone.add(k);
					const n =
						b.countLow >= 0
							? b.countLow + Math.floor(this.rnd() * (b.count - b.countLow + 1))
							: b.count;
					count += Math.max(0, n);
				}
			}
			const cap = Math.max(256, m.peak * 4);
			for (let i = 0; i < count && this.particles.length < cap; i++) this.spawn(local);
		}
		// Update.
		const alive: Particle[] = [];
		for (const p of this.particles) {
			p.age += dt;
			if (p.age >= p.life) continue;
			const rel = p.age / p.life;
			let vx = p.vel[0];
			let vy = p.vel[1];
			let vz = p.vel[2];
			if (m.velocityOverLife) {
				const s = sampleDist(m.velocityOverLife, rel, null, this.scratch);
				if (m.velocityOverLifeAbsolute) {
					vx = s[0];
					vy = s[1];
					vz = s[2];
				} else {
					vx *= s[0];
					vy *= s[1];
					vz *= s[2];
				}
			}
			p.pos[0] += vx * dt;
			p.pos[1] += vy * dt;
			p.pos[2] += vz * dt;
			p.vel[0] += p.accel[0] * dt;
			p.vel[1] += p.accel[1] * dt;
			p.vel[2] += p.accel[2] * dt;
			p.rot += p.rotRate * dt;
			for (let c = 0; c < 3; c++) {
				p.meshRot[c] += p.meshRate[c] * dt;
				p.orbitRot[c] += p.orbitRate[c] * dt;
			}
			// Size.
			p.size[0] = p.baseSize[0];
			p.size[1] = p.baseSize[1];
			p.size[2] = p.baseSize[2];
			if (m.sizeMultiplyLife) {
				const s = sampleDist(m.sizeMultiplyLife, rel, null, this.scratch);
				for (let c = 0; c < 3; c++) if (m.sizeMultiplyMask[c]) p.size[c] *= s[c];
			}
			if (m.sizeScale) {
				const s = sampleDist(m.sizeScale, rel, null, this.scratch);
				for (let c = 0; c < 3; c++) p.size[c] *= s[c];
			}
			// Colour.
			p.color[0] = p.baseColor[0];
			p.color[1] = p.baseColor[1];
			p.color[2] = p.baseColor[2];
			p.color[3] = p.baseColor[3];
			if (m.colorOverLife) {
				const s = sampleDist(m.colorOverLife, rel, null, this.scratch);
				p.color[0] = s[0];
				p.color[1] = s[1];
				p.color[2] = s[2];
			}
			if (m.alphaOverLife) {
				p.color[3] = sampleDist(m.alphaOverLife, rel, null, this.scratch)[0];
			}
			if (m.colorScaleOverLife) {
				const s = sampleDist(m.colorScaleOverLife, rel, null, this.scratch);
				p.color[0] *= s[0];
				p.color[1] *= s[1];
				p.color[2] *= s[2];
			}
			if (m.alphaScaleOverLife) {
				p.color[3] *= sampleDist(m.alphaScaleOverLife, rel, null, this.scratch)[0];
			}
			// Sub-UV.
			if (m.subImageIndex) {
				p.sub = sampleDist(m.subImageIndex, rel, null, this.scratch)[0];
			} else if (m.subMethod === "random" || m.subMethod === "random_blend") {
				p.sub = p.subRnd;
			} else if (m.subMethod !== "none") {
				p.sub = rel * m.subH * m.subV;
			}
			alive.push(p);
		}
		this.particles = alive;
	}

	private rnd3(): number[] {
		return [this.rnd(), this.rnd(), this.rnd()];
	}

	private spawn(et: number): void {
		const m = this.model;
		const s = this.scratch;
		const life = m.lifetime ? Math.max(0.01, sampleDist(m.lifetime, et, this.rnd3(), s)[0]) : 1;
		const p: Particle = {
			age: 0,
			life,
			pos: [0, 0, 0],
			vel: [0, 0, 0],
			baseSize: [1, 1, 1],
			size: [1, 1, 1],
			rot: 0,
			rotRate: 0,
			baseColor: [1, 1, 1, 1],
			color: [1, 1, 1, 1],
			sub: 0,
			subRnd: Math.floor(this.rnd() * m.subH * m.subV),
			orbit: [0, 0, 0],
			orbitRot: [0, 0, 0],
			orbitRate: [0, 0, 0],
			meshRot: [0, 0, 0],
			meshRate: [0, 0, 0],
			accel: [0, 0, 0],
		};
		for (const loc of m.locations) {
			sampleDist(loc, et, this.rnd3(), s);
			p.pos[0] += s[0];
			p.pos[1] += s[1];
			p.pos[2] += s[2];
		}
		if (m.sphere) {
			const r = m.sphere.radius ? sampleDist(m.sphere.radius, et, this.rnd3(), s)[0] : 0;
			const u = this.rnd() * TAU;
			const v = Math.acos(2 * this.rnd() - 1);
			const d = m.sphere.surface ? r : r * Math.cbrt(this.rnd());
			const dir = [Math.sin(v) * Math.cos(u), Math.sin(v) * Math.sin(u), Math.cos(v)];
			p.pos[0] += dir[0] * d;
			p.pos[1] += dir[1] * d;
			p.pos[2] += dir[2] * d;
			if (m.sphere.location) {
				sampleDist(m.sphere.location, et, this.rnd3(), s);
				p.pos[0] += s[0];
				p.pos[1] += s[1];
				p.pos[2] += s[2];
			}
			if (m.sphere.velocity) {
				const vs = m.sphere.velocityScale
					? sampleDist(m.sphere.velocityScale, et, this.rnd3(), s)[0]
					: 1;
				p.vel[0] += dir[0] * d * vs;
				p.vel[1] += dir[1] * d * vs;
				p.vel[2] += dir[2] * d * vs;
			}
		}
		if (m.cylinder) {
			const r = m.cylinder.radius ? sampleDist(m.cylinder.radius, et, this.rnd3(), s)[0] : 0;
			const h = m.cylinder.height ? sampleDist(m.cylinder.height, et, this.rnd3(), s)[0] : 0;
			const u = this.rnd() * TAU;
			const d = m.cylinder.surface ? r : r * Math.sqrt(this.rnd());
			const along = (this.rnd() - 0.5) * h;
			const radial: [number, number, number] = [0, 0, 0];
			let off: [number, number, number];
			if (m.cylinder.axis.endsWith("_x")) {
				radial[1] = Math.cos(u);
				radial[2] = Math.sin(u);
				off = [along, radial[1] * d, radial[2] * d];
			} else if (m.cylinder.axis.endsWith("_y")) {
				radial[0] = Math.cos(u);
				radial[2] = Math.sin(u);
				off = [radial[0] * d, along, radial[2] * d];
			} else {
				radial[0] = Math.cos(u);
				radial[1] = Math.sin(u);
				off = [radial[0] * d, radial[1] * d, along];
			}
			p.pos[0] += off[0];
			p.pos[1] += off[1];
			p.pos[2] += off[2];
			if (m.cylinder.location) {
				sampleDist(m.cylinder.location, et, this.rnd3(), s);
				p.pos[0] += s[0];
				p.pos[1] += s[1];
				p.pos[2] += s[2];
			}
			if (m.cylinder.velocity) {
				const vs = m.cylinder.velocityScale
					? sampleDist(m.cylinder.velocityScale, et, this.rnd3(), s)[0]
					: 1;
				p.vel[0] += radial[0] * d * vs;
				p.vel[1] += radial[1] * d * vs;
				p.vel[2] += radial[2] * d * vs;
			}
		}
		if (m.velocity) {
			sampleDist(m.velocity, et, this.rnd3(), s);
			p.vel[0] += s[0];
			p.vel[1] += s[1];
			p.vel[2] += s[2];
		}
		if (m.velocityRadial) {
			const vr = sampleDist(m.velocityRadial, et, this.rnd3(), s)[0];
			const len = Math.hypot(p.pos[0], p.pos[1], p.pos[2]);
			if (len > 1e-6 && vr !== 0) {
				p.vel[0] += (p.pos[0] / len) * vr;
				p.vel[1] += (p.pos[1] / len) * vr;
				p.vel[2] += (p.pos[2] / len) * vr;
			}
		}
		if (m.acceleration) {
			sampleDist(m.acceleration, et, this.rnd3(), s);
			p.accel = [s[0], s[1], s[2]];
		}
		if (m.size) {
			sampleDist(m.size, et, this.rnd3(), s);
			p.baseSize = [s[0], s[1], s[2]];
		}
		if (m.color) {
			sampleDist(m.color, et, this.rnd3(), s);
			p.baseColor[0] = s[0];
			p.baseColor[1] = s[1];
			p.baseColor[2] = s[2];
		}
		if (m.alpha) p.baseColor[3] = sampleDist(m.alpha, et, this.rnd3(), s)[0];
		if (m.rotation) p.rot = sampleDist(m.rotation, et, this.rnd3(), s)[0] * TAU;
		if (m.rotationRate) p.rotRate = sampleDist(m.rotationRate, et, this.rnd3(), s)[0] * TAU;
		if (m.orbitOffset) {
			sampleDist(m.orbitOffset, et, this.rnd3(), s);
			p.orbit = [s[0], s[1], s[2]];
		}
		if (m.orbitRotation) {
			sampleDist(m.orbitRotation, et, this.rnd3(), s);
			p.orbitRot = [s[0] * TAU, s[1] * TAU, s[2] * TAU];
		}
		if (m.orbitRotationRate) {
			sampleDist(m.orbitRotationRate, et, this.rnd3(), s);
			p.orbitRate = [s[0] * TAU, s[1] * TAU, s[2] * TAU];
		}
		if (m.meshRotation) {
			sampleDist(m.meshRotation, et, this.rnd3(), s);
			p.meshRot = [s[0] * TAU, s[1] * TAU, s[2] * TAU];
		}
		if (m.meshRotationRate) {
			sampleDist(m.meshRotationRate, et, this.rnd3(), s);
			p.meshRate = [s[0] * TAU, s[1] * TAU, s[2] * TAU];
		}
		p.size = [...p.baseSize];
		p.color = [...p.baseColor];
		this.particles.push(p);
	}
}

/** A small deterministic PRNG so the preview replays the same way. */
export function mulberry(seed: number): () => number {
	let a = seed >>> 0 || 1;
	return () => {
		a += 0x6d2b79f5;
		let t = a;
		t = Math.imul(t ^ (t >>> 15), t | 1);
		t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

/** Orbit offset of a particle in Unreal space (rotation around the particle's own origin). */
export function orbitOffset(p: Particle, out: [number, number, number]): [number, number, number] {
	let [x, y, z] = p.orbit;
	const [rx, ry, rz] = p.orbitRot;
	// Z, then Y, then X rotations.
	let c = Math.cos(rz);
	let s = Math.sin(rz);
	[x, y] = [x * c - y * s, x * s + y * c];
	c = Math.cos(ry);
	s = Math.sin(ry);
	[x, z] = [x * c + z * s, -x * s + z * c];
	c = Math.cos(rx);
	s = Math.sin(rx);
	[y, z] = [y * c - z * s, y * s + z * c];
	out[0] = x;
	out[1] = y;
	out[2] = z;
	return out;
}
