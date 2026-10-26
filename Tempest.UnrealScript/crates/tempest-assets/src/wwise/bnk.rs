//! Wwise soundbanks (`BKHD` chunks, version 120): embedded media
//! (`DIDX` index + `DATA`), and the object hierarchy (`HIRC`) as far as
//! finding what an event plays: event → actions → target → sounds and
//! music tracks → media ids.
//!
//! The hierarchy objects are kept as opaque bodies; only the fields at
//! fixed offsets are read (ids, a sound's source, an event's actions, an
//! action's type and target). Children of containers are found by looking
//! for the container's id inside candidate objects (every node stores its
//! parent id in `NodeBaseParams`, whose layout is version-specific).

use std::collections::{HashSet, VecDeque};

use tempest_upk::io::{Error, Result};

pub const SOUND: u8 = 2;
pub const ACTION: u8 = 3;
pub const EVENT: u8 = 4;
pub const RANDOM_SEQUENCE: u8 = 5;
pub const SWITCH: u8 = 6;
pub const ACTOR_MIXER: u8 = 7;
pub const LAYER: u8 = 9;
pub const MUSIC_SEGMENT: u8 = 10;
pub const MUSIC_TRACK: u8 = 11;
pub const MUSIC_SWITCH: u8 = 12;
pub const MUSIC_RANDOM_SEQUENCE: u8 = 13;

pub const PLUGIN_VORBIS: u32 = 0x0004_0001;

pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        1 => "settings",
        SOUND => "sound",
        ACTION => "action",
        EVENT => "event",
        RANDOM_SEQUENCE => "random/sequence container",
        SWITCH => "switch container",
        ACTOR_MIXER => "actor-mixer",
        8 => "bus",
        LAYER => "layer container",
        MUSIC_SEGMENT => "music segment",
        MUSIC_TRACK => "music track",
        MUSIC_SWITCH => "music switch",
        MUSIC_RANDOM_SEQUENCE => "music playlist",
        14 => "attenuation",
        15 => "dialogue event",
        18 => "effect",
        19 => "aux bus",
        20 => "LFO",
        21 => "envelope",
        22 => "audio device",
        _ => "object",
    }
}

#[derive(Clone, Debug)]
pub struct Object {
    pub kind: u8,
    pub id: u32,
    /// The body after the id.
    pub body: Vec<u8>,
}

/// A media source referenced by a sound or music track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub plugin: u32,
    /// 0 = in the bank, 1 = prefetched, 2 = streamed.
    pub stream_type: u8,
    pub id: u32,
    pub in_memory_size: u32,
}

impl Source {
    fn read(b: &[u8], at: usize) -> Option<(Source, usize)> {
        let plugin = u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?);
        let stream_type = *b.get(at + 4)?;
        let id = u32::from_le_bytes(b.get(at + 5..at + 9)?.try_into().ok()?);
        let in_memory_size = u32::from_le_bytes(b.get(at + 9..at + 13)?.try_into().ok()?);
        Some((Source { plugin, stream_type, id, in_memory_size }, at + 14))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Bank {
    pub version: u32,
    pub id: u32,
    /// Chunks in file order (`BKHD` first).
    pub chunks: Vec<([u8; 4], Vec<u8>)>,
}

impl Bank {
    pub fn parse(bytes: &[u8]) -> Result<Bank> {
        let mut pos = 0;
        let mut chunks = Vec::new();
        while pos + 8 <= bytes.len() {
            let id: [u8; 4] = bytes[pos..pos + 4].try_into().unwrap();
            let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let body = bytes.get(pos + 8..pos + 8 + size).ok_or_else(|| Error::Format("BNK: chunk truncated".into()))?;
            chunks.push((id, body.to_vec()));
            pos += 8 + size;
        }
        let bkhd = chunks.iter().find(|(id, _)| id == b"BKHD").ok_or_else(|| Error::Format("not a Wwise soundbank (no BKHD)".into()))?;
        if bkhd.1.len() < 8 {
            return Err(Error::Format("BNK: short BKHD".into()));
        }
        Ok(Bank { version: u32::from_le_bytes(bkhd.1[0..4].try_into().unwrap()), id: u32::from_le_bytes(bkhd.1[4..8].try_into().unwrap()), chunks })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (id, body) in &self.chunks {
            out.extend_from_slice(id);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(body);
        }
        out
    }

    fn chunk(&self, id: &[u8; 4]) -> Option<&[u8]> {
        self.chunks.iter().find(|(i, _)| i == id).map(|(_, b)| b.as_slice())
    }

    /// Embedded media: `(id, bytes)` in index order.
    pub fn media(&self) -> Vec<(u32, &[u8])> {
        let (Some(didx), Some(data)) = (self.chunk(b"DIDX"), self.chunk(b"DATA")) else { return Vec::new() };
        didx.chunks_exact(12)
            .filter_map(|e| {
                let id = u32::from_le_bytes(e[0..4].try_into().unwrap());
                let off = u32::from_le_bytes(e[4..8].try_into().unwrap()) as usize;
                let size = u32::from_le_bytes(e[8..12].try_into().unwrap()) as usize;
                data.get(off..off + size).map(|d| (id, d))
            })
            .collect()
    }

    /// Replace an embedded media file: the index and data are rebuilt
    /// (16-byte aligned, as the cooker lays them out) and every sound or
    /// music track sourcing `id` from the bank gets the new in-memory size.
    pub fn replace_media(&mut self, id: u32, bytes: &[u8]) -> Result<()> {
        let entries: Vec<(u32, Vec<u8>)> = self.media().into_iter().map(|(i, d)| (i, if i == id { bytes.to_vec() } else { d.to_vec() })).collect();
        if !entries.iter().any(|(i, _)| *i == id) {
            return Err(Error::Format(format!("media {id} is not embedded in bank {:08x}", self.id)));
        }
        let mut didx = Vec::with_capacity(entries.len() * 12);
        let mut data = Vec::new();
        for (i, d) in &entries {
            let off = data.len().div_ceil(16) * 16;
            data.resize(off, 0);
            didx.extend_from_slice(&i.to_le_bytes());
            didx.extend_from_slice(&(off as u32).to_le_bytes());
            didx.extend_from_slice(&(d.len() as u32).to_le_bytes());
            data.extend_from_slice(d);
        }
        for (cid, body) in self.chunks.iter_mut() {
            if cid == b"DIDX" {
                *body = didx.clone();
            } else if cid == b"DATA" {
                *body = data.clone();
            }
        }
        // In-memory size on the sources.
        let objects = self.objects();
        let mut patches: Vec<(usize, u32)> = Vec::new();
        for (index, o) in objects.iter().enumerate() {
            for (src, at) in sources_with_offsets(o) {
                if src.id == id && src.stream_type != 2 {
                    patches.push((index, at as u32));
                }
            }
        }
        if !patches.is_empty() {
            let Some((_, hirc)) = self.chunks.iter_mut().find(|(i, _)| i == b"HIRC") else { return Ok(()) };
            let mut pos = 4;
            let mut index = 0;
            while pos + 5 <= hirc.len() {
                let size = u32::from_le_bytes(hirc[pos + 1..pos + 5].try_into().unwrap()) as usize;
                for (pi, at) in &patches {
                    if *pi == index {
                        // `at` is the source offset within the body (after the id): in-memory size at +9.
                        let p = pos + 5 + 4 + *at as usize + 9;
                        if p + 4 <= hirc.len() {
                            hirc[p..p + 4].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                        }
                    }
                }
                pos += 5 + size;
                index += 1;
            }
        }
        Ok(())
    }

    /// The hierarchy objects.
    pub fn objects(&self) -> Vec<Object> {
        let Some(hirc) = self.chunk(b"HIRC") else { return Vec::new() };
        if hirc.len() < 4 {
            return Vec::new();
        }
        let n = u32::from_le_bytes(hirc[0..4].try_into().unwrap()) as usize;
        let mut out = Vec::with_capacity(n);
        let mut pos = 4;
        for _ in 0..n {
            if pos + 9 > hirc.len() {
                break;
            }
            let kind = hirc[pos];
            let size = u32::from_le_bytes(hirc[pos + 1..pos + 5].try_into().unwrap()) as usize;
            let id = u32::from_le_bytes(hirc[pos + 5..pos + 9].try_into().unwrap());
            let Some(body) = hirc.get(pos + 9..pos + 5 + size) else { break };
            out.push(Object { kind, id, body: body.to_vec() });
            pos += 5 + size;
        }
        out
    }
}

/// Sources of a sound (one) or a music track (several), with the offset of
/// each `AkBankSourceData` inside the body.
fn sources_with_offsets(o: &Object) -> Vec<(Source, usize)> {
    match o.kind {
        SOUND => Source::read(&o.body, 0).map(|(s, _)| vec![(s, 0)]).unwrap_or_default(),
        MUSIC_TRACK => {
            // uFlags u8, numSources u32, sources.
            let Some(n) = o.body.get(1..5).map(|b| u32::from_le_bytes(b.try_into().unwrap())) else { return Vec::new() };
            let mut at = 5;
            let mut out = Vec::new();
            for _ in 0..n.min(64) {
                let Some((s, next)) = Source::read(&o.body, at) else { break };
                out.push((s, at));
                at = next;
            }
            out
        }
        _ => Vec::new(),
    }
}

/// Sources of a sound or music track.
pub fn sources(o: &Object) -> Vec<Source> {
    sources_with_offsets(o).into_iter().map(|(s, _)| s).collect()
}

/// An event's action ids.
pub fn event_actions(o: &Object) -> Vec<u32> {
    if o.kind != EVENT || o.body.len() < 4 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(o.body[0..4].try_into().unwrap()) as usize;
    (0..n.min(256)).filter_map(|i| o.body.get(4 + i * 4..8 + i * 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))).collect()
}

/// An action's `(type, target id)`; `0x04xx` types play.
pub fn action(o: &Object) -> Option<(u16, u32)> {
    if o.kind != ACTION || o.body.len() < 6 {
        return None;
    }
    Some((u16::from_le_bytes([o.body[0], o.body[1]]), u32::from_le_bytes(o.body[2..6].try_into().unwrap())))
}

pub fn action_name(kind: u16) -> &'static str {
    match kind >> 8 {
        0x01 => "stop",
        0x02 => "pause",
        0x03 => "resume",
        0x04 => "play",
        0x05 => "play and continue",
        0x06 => "mute",
        0x07 => "unmute",
        0x08 => "set pitch",
        0x09 => "reset pitch",
        0x0A => "set volume",
        0x0B => "reset volume",
        0x0C => "set bus volume",
        0x0D => "reset bus volume",
        0x0E => "set low-pass",
        0x0F => "reset low-pass",
        0x10 => "stop",
        0x11 => "use state",
        0x12 => "set state",
        0x13 => "set game parameter",
        0x14 => "reset game parameter",
        0x19 => "set switch",
        0x1A => "set state",
        0x1B => "bypass effect",
        0x1C => "reset bypass effect",
        0x1D => "break",
        0x1E => "trigger",
        0x1F => "seek",
        0x20 => "release",
        0x21 => "set high-pass",
        0x22 => "reset high-pass",
        _ => "action",
    }
}

fn contains_id(body: &[u8], id: u32) -> bool {
    let needle = id.to_le_bytes();
    body.windows(4).skip(4).any(|w| w == needle)
}

/// Media ids an event plays, following play actions into containers,
/// music playlists, segments and tracks. The second list names the
/// objects visited, for display.
pub fn event_media(objects: &[Object], event: u32) -> (Vec<Source>, Vec<(u8, u32)>) {
    let find = |id: u32| objects.iter().find(|o| o.id == id);
    let mut out: Vec<Source> = Vec::new();
    let mut visited: HashSet<u32> = HashSet::new();
    let mut trail = Vec::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    if let Some(e) = find(event) {
        for a in event_actions(e) {
            if let Some(act) = find(a) {
                if let Some((kind, target)) = action(act) {
                    if kind >> 8 == 0x04 || kind >> 8 == 0x05 {
                        queue.push_back(target);
                    }
                }
            }
        }
    }
    while let Some(id) = queue.pop_front() {
        if id == 0 || !visited.insert(id) {
            continue;
        }
        let Some(o) = find(id) else { continue };
        trail.push((o.kind, o.id));
        match o.kind {
            SOUND | MUSIC_TRACK => {
                for s in sources(o) {
                    if !out.contains(&s) {
                        out.push(s);
                    }
                }
            }
            RANDOM_SEQUENCE | SWITCH | ACTOR_MIXER | LAYER | MUSIC_SEGMENT | MUSIC_SWITCH | MUSIC_RANDOM_SEQUENCE => {
                // What this kind of node may contain (its parent also holds
                // its id, so the kinds keep the search going downwards).
                let child_kinds: &[u8] = match o.kind {
                    ACTOR_MIXER => &[SOUND, RANDOM_SEQUENCE, SWITCH, LAYER, ACTOR_MIXER],
                    MUSIC_SEGMENT => &[MUSIC_TRACK],
                    MUSIC_SWITCH | MUSIC_RANDOM_SEQUENCE => &[MUSIC_SEGMENT, MUSIC_SWITCH, MUSIC_RANDOM_SEQUENCE],
                    _ => &[SOUND, RANDOM_SEQUENCE, SWITCH, LAYER],
                };
                for c in objects.iter().filter(|c| c.id != id && child_kinds.contains(&c.kind) && contains_id(&c.body, id)) {
                    queue.push_back(c.id);
                }
            }
            _ => {}
        }
    }
    (out, trail)
}
