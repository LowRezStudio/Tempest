//! Wwise Vorbis (`.wem`, format tag 0xFFFF) ⇄ standard Ogg Vorbis.
//!
//! Wwise stores a Vorbis stream with its own framing: a 66-byte `fmt `
//! (`AkVorbisInfo` after the WAVEFORMATEX), then a `data` chunk of
//! `u16 size` + payload packets. The first packet is a stripped setup
//! header: codebooks are 10-bit ids into a fixed library (aoTuV 6.03's
//! static books, `data/packed_codebooks_aoTuV_603.bin`), floor/residue/
//! mapping/mode tables lose their constant fields. Audio packets lose the
//! packet-type bit and the two window flags ("mod packets"). The algorithm
//! is ww2ogg's (Adam Gashlin); the reverse direction matches a standard
//! stream's codebooks against the library, so only streams encoded by the
//! same libvorbis flavour (the aoTuV encoder) convert.
//!
//! The fields of the 66-byte header, as this game writes them (every one
//! of its 43 489 streams): channels, rate, average bytes per second, a
//! channel configuration (`mask << 12 | 1 << 8 | channels`), total PCM
//! frames, loop start/end packet offsets (first audio packet / data size
//! without a loop), loop extras, seek table size, Vorbis data offset, max
//! packet size, last-granule excess, two decoder allocation sizes, the
//! FNV-1 hash of the setup packet, the two block size exponents.

use std::sync::OnceLock;

use tempest_upk::io::{Error, Result};

use super::fnv::fnv1_32;

const CODEBOOKS: &[u8] = include_bytes!("../../data/packed_codebooks_aoTuV_603.bin");

// ----------------------------------------------------------------------------
// Bit I/O (LSB first, as Vorbis)
// ----------------------------------------------------------------------------

pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }

    pub fn bit(&mut self) -> Result<bool> {
        let byte = self.data.get(self.pos / 8).ok_or_else(|| Error::Format("Vorbis: out of bits".into()))?;
        let b = (byte >> (self.pos % 8)) & 1 != 0;
        self.pos += 1;
        Ok(b)
    }

    pub fn bits(&mut self, n: u32) -> Result<u32> {
        let mut v = 0u32;
        for i in 0..n {
            if self.bit()? {
                v |= 1 << i;
            }
        }
        Ok(v)
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn total_bits(&self) -> usize {
        self.data.len() * 8
    }
}

#[derive(Default)]
pub struct BitWriter {
    out: Vec<u8>,
    nbits: usize,
}

impl BitWriter {
    pub fn bit(&mut self, b: bool) {
        if self.nbits.is_multiple_of(8) {
            self.out.push(0);
        }
        if b {
            let i = self.out.len() - 1;
            self.out[i] |= 1 << (self.nbits % 8);
        }
        self.nbits += 1;
    }

    pub fn bits(&mut self, v: u32, n: u32) {
        for i in 0..n {
            self.bit((v >> i) & 1 != 0);
        }
    }

    pub fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.bits(x as u32, 8);
        }
    }

    pub fn finish(self) -> Vec<u8> {
        self.out
    }

    pub fn len_bits(&self) -> usize {
        self.nbits
    }
}

fn ilog(mut v: u32) -> u32 {
    let mut r = 0;
    while v != 0 {
        r += 1;
        v >>= 1;
    }
    r
}

/// `_book_maptype1_quantvals` from Tremor.
fn maptype1_quantvals(entries: u32, dimensions: u32) -> u32 {
    let bits = ilog(entries);
    let mut vals = entries >> ((bits - 1) * (dimensions - 1) / dimensions);
    loop {
        let mut acc = 1u64;
        let mut acc1 = 1u64;
        for _ in 0..dimensions {
            acc *= vals as u64;
            acc1 *= vals as u64 + 1;
        }
        if acc <= entries as u64 && acc1 > entries as u64 {
            return vals;
        } else if acc > entries as u64 {
            vals -= 1;
        } else {
            vals += 1;
        }
    }
}

// ----------------------------------------------------------------------------
// Codebooks
// ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lookup {
    pub min: u32,
    pub max: u32,
    pub value_bits: u32,
    pub sequence: bool,
    pub values: Vec<u32>,
}

/// A codebook in canonical form, whatever the syntax it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Codebook {
    pub dimensions: u32,
    pub entries: u32,
    /// Codeword length − 1 per entry; `None` for an unused (sparse) entry.
    pub lengths: Vec<Option<u32>>,
    pub lookup: Option<Lookup>,
}

impl Codebook {
    fn read_lengths_ordered(r: &mut BitReader, entries: u32) -> Result<Vec<Option<u32>>> {
        let mut lengths = Vec::with_capacity(entries as usize);
        let mut current_length = r.bits(5)?;
        let mut current = 0u32;
        while current < entries {
            let number = r.bits(ilog(entries - current))?;
            for _ in 0..number {
                lengths.push(Some(current_length));
            }
            current += number;
            current_length += 1;
        }
        if current > entries {
            return Err(Error::Format("Vorbis: ordered codebook entry count out of range".into()));
        }
        Ok(lengths)
    }

    fn read_lookup(r: &mut BitReader, lookup_type: u32, entries: u32, dimensions: u32) -> Result<Option<Lookup>> {
        match lookup_type {
            0 => Ok(None),
            1 => {
                let min = r.bits(32)?;
                let max = r.bits(32)?;
                let value_bits = r.bits(4)? + 1;
                let sequence = r.bit()?;
                let n = maptype1_quantvals(entries, dimensions);
                let mut values = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    values.push(r.bits(value_bits)?);
                }
                Ok(Some(Lookup { min, max, value_bits, sequence, values }))
            }
            2 => Err(Error::Format("Vorbis: lookup type 2 codebook".into())),
            _ => Err(Error::Format("Vorbis: invalid codebook lookup type".into())),
        }
    }

    /// Wwise's packed form (as in the library and inline codebooks).
    pub fn parse_packed(r: &mut BitReader) -> Result<Codebook> {
        let dimensions = r.bits(4)?;
        let entries = r.bits(14)?;
        let ordered = r.bit()?;
        let lengths = if ordered {
            Self::read_lengths_ordered(r, entries)?
        } else {
            let length_bits = r.bits(3)?;
            let sparse = r.bit()?;
            if length_bits == 0 || length_bits > 5 {
                return Err(Error::Format("Vorbis: nonsense codeword length".into()));
            }
            let mut lengths = Vec::with_capacity(entries as usize);
            for _ in 0..entries {
                let present = if sparse { r.bit()? } else { true };
                lengths.push(if present { Some(r.bits(length_bits)?) } else { None });
            }
            lengths
        };
        let lookup_type = r.bits(1)?;
        let lookup = Self::read_lookup(r, lookup_type, entries, dimensions)?;
        Ok(Codebook { dimensions, entries, lengths, lookup })
    }

    /// The standard Vorbis setup-header form (`BCV` sync pattern).
    pub fn parse_full(r: &mut BitReader) -> Result<Codebook> {
        if r.bits(24)? != 0x564342 {
            return Err(Error::Format("Vorbis: invalid codebook identifier".into()));
        }
        let dimensions = r.bits(16)?;
        let entries = r.bits(24)?;
        let ordered = r.bit()?;
        let lengths = if ordered {
            Self::read_lengths_ordered(r, entries)?
        } else {
            let sparse = r.bit()?;
            let mut lengths = Vec::with_capacity(entries as usize);
            for _ in 0..entries {
                let present = if sparse { r.bit()? } else { true };
                lengths.push(if present { Some(r.bits(5)?) } else { None });
            }
            lengths
        };
        let lookup_type = r.bits(4)?;
        let lookup = Self::read_lookup(r, lookup_type, entries, dimensions)?;
        Ok(Codebook { dimensions, entries, lengths, lookup })
    }

    /// Ordered lengths: all present and never decreasing.
    fn ordered_runs(&self) -> Option<Vec<(u32, u32)>> {
        let mut runs: Vec<(u32, u32)> = Vec::new();
        let mut prev: Option<u32> = None;
        for l in &self.lengths {
            let l = (*l)?;
            match prev {
                Some(p) if l < p => return None,
                _ => {}
            }
            match runs.last_mut() {
                Some((len, n)) if *len == l => *n += 1,
                _ => runs.push((l, 1)),
            }
            prev = Some(l);
        }
        // The ordered syntax steps the length by exactly one per run.
        if runs.windows(2).any(|w| w[1].0 != w[0].0 + 1) {
            return None;
        }
        Some(runs)
    }

    /// Write the standard form.
    pub fn write_full(&self, w: &mut BitWriter) {
        w.bits(0x564342, 24);
        w.bits(self.dimensions, 16);
        w.bits(self.entries, 24);
        if let Some(runs) = self.ordered_runs() {
            w.bit(true);
            w.bits(runs[0].0, 5);
            let mut current = 0u32;
            for (_, n) in runs {
                w.bits(n, ilog(self.entries - current));
                current += n;
            }
        } else {
            w.bit(false);
            let sparse = self.lengths.iter().any(|l| l.is_none());
            w.bit(sparse);
            for l in &self.lengths {
                if sparse {
                    w.bit(l.is_some());
                }
                if let Some(l) = l {
                    w.bits(*l, 5);
                }
            }
        }
        match &self.lookup {
            None => w.bits(0, 4),
            Some(l) => {
                w.bits(1, 4);
                w.bits(l.min, 32);
                w.bits(l.max, 32);
                w.bits(l.value_bits - 1, 4);
                w.bit(l.sequence);
                for v in &l.values {
                    w.bits(*v, l.value_bits);
                }
            }
        }
    }
}

/// The codebook library Wwise streams reference by id.
pub struct Library {
    books: Vec<Codebook>,
}

impl Library {
    fn load() -> Library {
        let n = CODEBOOKS.len();
        let offset_offset = u32::from_le_bytes([CODEBOOKS[n - 4], CODEBOOKS[n - 3], CODEBOOKS[n - 2], CODEBOOKS[n - 1]]) as usize;
        let count = (n - offset_offset) / 4;
        let offsets: Vec<usize> = (0..count).map(|i| u32::from_le_bytes(CODEBOOKS[offset_offset + i * 4..offset_offset + i * 4 + 4].try_into().unwrap()) as usize).collect();
        let mut books = Vec::with_capacity(count.saturating_sub(1));
        for i in 0..count.saturating_sub(1) {
            let slice = &CODEBOOKS[offsets[i]..offsets[i + 1]];
            let mut r = BitReader::new(slice);
            match Codebook::parse_packed(&mut r) {
                Ok(b) => books.push(b),
                Err(_) => break,
            }
        }
        Library { books }
    }

    pub fn get() -> &'static Library {
        static LIB: OnceLock<Library> = OnceLock::new();
        LIB.get_or_init(Library::load)
    }

    pub fn book(&self, id: usize) -> Option<&Codebook> {
        self.books.get(id)
    }

    pub fn len(&self) -> usize {
        self.books.len()
    }

    pub fn is_empty(&self) -> bool {
        self.books.is_empty()
    }

    pub fn find(&self, book: &Codebook) -> Option<usize> {
        self.books.iter().position(|b| b == book)
    }
}

// ----------------------------------------------------------------------------
// Setup header: packed (Wwise) ⇄ full (Vorbis)
// ----------------------------------------------------------------------------

/// What the audio packets need from the setup: block flags per mode.
#[derive(Clone, Debug)]
pub struct Modes {
    pub blockflag: Vec<bool>,
    pub mode_bits: u32,
}

/// Convert a packed setup packet into a full Vorbis setup header body (the
/// bits after `\x05vorbis`), returning the modes.
fn unpack_setup(packed: &[u8], channels: u32, w: &mut BitWriter) -> Result<Modes> {
    let lib = Library::get();
    let mut r = BitReader::new(packed);
    let codebook_count = r.bits(8)? + 1;
    w.bits(codebook_count - 1, 8);
    for _ in 0..codebook_count {
        let id = r.bits(10)? as usize;
        let book = lib.book(id).ok_or_else(|| Error::Format(format!("Wwise Vorbis: codebook id {id} is not in the library")))?;
        book.write_full(w);
    }
    // Time domain transforms (placeholder).
    w.bits(0, 6);
    w.bits(0, 16);
    // Floors.
    let floor_count = r.bits(6)? + 1;
    w.bits(floor_count - 1, 6);
    for _ in 0..floor_count {
        w.bits(1, 16);
        let partitions = r.bits(5)?;
        w.bits(partitions, 5);
        let mut classes = Vec::with_capacity(partitions as usize);
        let mut max_class = 0;
        for _ in 0..partitions {
            let c = r.bits(4)?;
            w.bits(c, 4);
            max_class = max_class.max(c);
            classes.push(c);
        }
        let mut class_dims = vec![0u32; max_class as usize + 1];
        for dims in class_dims.iter_mut() {
            let d = r.bits(3)?;
            w.bits(d, 3);
            *dims = d + 1;
            let subclasses = r.bits(2)?;
            w.bits(subclasses, 2);
            if subclasses != 0 {
                let masterbook = r.bits(8)?;
                w.bits(masterbook, 8);
                if masterbook >= codebook_count {
                    return Err(Error::Format("Vorbis: invalid floor1 masterbook".into()));
                }
            }
            for _ in 0..(1u32 << subclasses) {
                let book_plus1 = r.bits(8)?;
                w.bits(book_plus1, 8);
                if book_plus1 > codebook_count {
                    return Err(Error::Format("Vorbis: invalid floor1 subclass book".into()));
                }
            }
        }
        let multiplier = r.bits(2)?;
        w.bits(multiplier, 2);
        let rangebits = r.bits(4)?;
        w.bits(rangebits, 4);
        for &c in &classes {
            for _ in 0..class_dims[c as usize] {
                let x = r.bits(rangebits)?;
                w.bits(x, rangebits);
            }
        }
    }
    // Residues.
    let residue_count = r.bits(6)? + 1;
    w.bits(residue_count - 1, 6);
    for _ in 0..residue_count {
        let residue_type = r.bits(2)?;
        w.bits(residue_type, 16);
        if residue_type > 2 {
            return Err(Error::Format("Vorbis: invalid residue type".into()));
        }
        let begin = r.bits(24)?;
        let end = r.bits(24)?;
        let partition_size = r.bits(24)?;
        let classifications = r.bits(6)? + 1;
        let classbook = r.bits(8)?;
        w.bits(begin, 24);
        w.bits(end, 24);
        w.bits(partition_size, 24);
        w.bits(classifications - 1, 6);
        w.bits(classbook, 8);
        if classbook >= codebook_count {
            return Err(Error::Format("Vorbis: invalid residue classbook".into()));
        }
        let mut cascade = Vec::with_capacity(classifications as usize);
        for _ in 0..classifications {
            let low = r.bits(3)?;
            w.bits(low, 3);
            let flag = r.bit()?;
            w.bit(flag);
            let high = if flag {
                let h = r.bits(5)?;
                w.bits(h, 5);
                h
            } else {
                0
            };
            cascade.push(high * 8 + low);
        }
        for &c in &cascade {
            for k in 0..8 {
                if c & (1 << k) != 0 {
                    let book = r.bits(8)?;
                    w.bits(book, 8);
                    if book >= codebook_count {
                        return Err(Error::Format("Vorbis: invalid residue book".into()));
                    }
                }
            }
        }
    }
    // Mappings.
    let mapping_count = r.bits(6)? + 1;
    w.bits(mapping_count - 1, 6);
    for _ in 0..mapping_count {
        w.bits(0, 16);
        let submaps_flag = r.bit()?;
        w.bit(submaps_flag);
        let mut submaps = 1;
        if submaps_flag {
            let s = r.bits(4)?;
            w.bits(s, 4);
            submaps = s + 1;
        }
        let square_polar = r.bit()?;
        w.bit(square_polar);
        if square_polar {
            let steps = r.bits(8)? + 1;
            w.bits(steps - 1, 8);
            let bits = ilog(channels - 1);
            for _ in 0..steps {
                let magnitude = r.bits(bits)?;
                let angle = r.bits(bits)?;
                w.bits(magnitude, bits);
                w.bits(angle, bits);
                if angle == magnitude || magnitude >= channels || angle >= channels {
                    return Err(Error::Format("Vorbis: invalid coupling".into()));
                }
            }
        }
        let reserved = r.bits(2)?;
        w.bits(reserved, 2);
        if reserved != 0 {
            return Err(Error::Format("Vorbis: mapping reserved field nonzero".into()));
        }
        if submaps > 1 {
            for _ in 0..channels {
                let mux = r.bits(4)?;
                w.bits(mux, 4);
                if mux >= submaps {
                    return Err(Error::Format("Vorbis: mapping mux out of range".into()));
                }
            }
        }
        for _ in 0..submaps {
            let time = r.bits(8)?;
            w.bits(time, 8);
            let floor = r.bits(8)?;
            w.bits(floor, 8);
            let residue = r.bits(8)?;
            w.bits(residue, 8);
            if floor >= floor_count || residue >= residue_count {
                return Err(Error::Format("Vorbis: invalid submap floor/residue".into()));
            }
        }
    }
    // Modes.
    let mode_count = r.bits(6)? + 1;
    w.bits(mode_count - 1, 6);
    let mut blockflag = Vec::with_capacity(mode_count as usize);
    for _ in 0..mode_count {
        let flag = r.bit()?;
        w.bit(flag);
        blockflag.push(flag);
        w.bits(0, 16);
        w.bits(0, 16);
        let mapping = r.bits(8)?;
        w.bits(mapping, 8);
        if mapping >= mapping_count {
            return Err(Error::Format("Vorbis: invalid mode mapping".into()));
        }
    }
    w.bit(true);
    if r.position().div_ceil(8) != packed.len() {
        return Err(Error::Format("Wwise Vorbis: setup packet size mismatch".into()));
    }
    Ok(Modes { blockflag, mode_bits: ilog(mode_count - 1) })
}

/// Convert a full Vorbis setup header body (after `\x05vorbis`) into
/// Wwise's packed form, returning the modes. Every codebook must be in the
/// library.
fn pack_setup(full: &[u8], channels: u32) -> Result<(Vec<u8>, Modes)> {
    let lib = Library::get();
    let mut r = BitReader::new(full);
    let mut w = BitWriter::default();
    let codebook_count = r.bits(8)? + 1;
    w.bits(codebook_count - 1, 8);
    for i in 0..codebook_count {
        let book = Codebook::parse_full(&mut r)?;
        let id = lib.find(&book).ok_or_else(|| Error::Format(format!("codebook {i} ({} entries × {}) is not in the Wwise library: the stream must come from the aoTuV encoder", book.entries, book.dimensions)))?;
        w.bits(id as u32, 10);
    }
    let time_count = r.bits(6)? + 1;
    for _ in 0..time_count {
        if r.bits(16)? != 0 {
            return Err(Error::Format("Vorbis: nonzero time domain transform".into()));
        }
    }
    let floor_count = r.bits(6)? + 1;
    w.bits(floor_count - 1, 6);
    for _ in 0..floor_count {
        if r.bits(16)? != 1 {
            return Err(Error::Format("Vorbis: floor type 0 is not supported by Wwise".into()));
        }
        let partitions = r.bits(5)?;
        w.bits(partitions, 5);
        let mut classes = Vec::with_capacity(partitions as usize);
        let mut max_class = 0;
        for _ in 0..partitions {
            let c = r.bits(4)?;
            w.bits(c, 4);
            max_class = max_class.max(c);
            classes.push(c);
        }
        let mut class_dims = vec![0u32; max_class as usize + 1];
        for dims in class_dims.iter_mut() {
            let d = r.bits(3)?;
            w.bits(d, 3);
            *dims = d + 1;
            let subclasses = r.bits(2)?;
            w.bits(subclasses, 2);
            if subclasses != 0 {
                w.bits(r.bits(8)?, 8);
            }
            for _ in 0..(1u32 << subclasses) {
                w.bits(r.bits(8)?, 8);
            }
        }
        w.bits(r.bits(2)?, 2);
        let rangebits = r.bits(4)?;
        w.bits(rangebits, 4);
        for &c in &classes {
            for _ in 0..class_dims[c as usize] {
                w.bits(r.bits(rangebits)?, rangebits);
            }
        }
    }
    let residue_count = r.bits(6)? + 1;
    w.bits(residue_count - 1, 6);
    for _ in 0..residue_count {
        let residue_type = r.bits(16)?;
        if residue_type > 2 {
            return Err(Error::Format("Vorbis: invalid residue type".into()));
        }
        w.bits(residue_type, 2);
        w.bits(r.bits(24)?, 24);
        w.bits(r.bits(24)?, 24);
        w.bits(r.bits(24)?, 24);
        let classifications = r.bits(6)? + 1;
        w.bits(classifications - 1, 6);
        w.bits(r.bits(8)?, 8);
        let mut cascade = Vec::with_capacity(classifications as usize);
        for _ in 0..classifications {
            let low = r.bits(3)?;
            w.bits(low, 3);
            let flag = r.bit()?;
            w.bit(flag);
            let high = if flag {
                let h = r.bits(5)?;
                w.bits(h, 5);
                h
            } else {
                0
            };
            cascade.push(high * 8 + low);
        }
        for &c in &cascade {
            for k in 0..8 {
                if c & (1 << k) != 0 {
                    w.bits(r.bits(8)?, 8);
                }
            }
        }
    }
    let mapping_count = r.bits(6)? + 1;
    w.bits(mapping_count - 1, 6);
    for _ in 0..mapping_count {
        if r.bits(16)? != 0 {
            return Err(Error::Format("Vorbis: invalid mapping type".into()));
        }
        let submaps_flag = r.bit()?;
        w.bit(submaps_flag);
        let mut submaps = 1;
        if submaps_flag {
            let s = r.bits(4)?;
            w.bits(s, 4);
            submaps = s + 1;
        }
        let square_polar = r.bit()?;
        w.bit(square_polar);
        if square_polar {
            let steps = r.bits(8)? + 1;
            w.bits(steps - 1, 8);
            let bits = ilog(channels - 1);
            for _ in 0..steps {
                w.bits(r.bits(bits)?, bits);
                w.bits(r.bits(bits)?, bits);
            }
        }
        w.bits(r.bits(2)?, 2);
        if submaps > 1 {
            for _ in 0..channels {
                w.bits(r.bits(4)?, 4);
            }
        }
        for _ in 0..submaps {
            w.bits(r.bits(8)?, 8);
            w.bits(r.bits(8)?, 8);
            w.bits(r.bits(8)?, 8);
        }
    }
    let mode_count = r.bits(6)? + 1;
    w.bits(mode_count - 1, 6);
    let mut blockflag = Vec::with_capacity(mode_count as usize);
    for _ in 0..mode_count {
        let flag = r.bit()?;
        w.bit(flag);
        blockflag.push(flag);
        if r.bits(16)? != 0 || r.bits(16)? != 0 {
            return Err(Error::Format("Vorbis: nonzero window/transform type".into()));
        }
        w.bits(r.bits(8)?, 8);
    }
    if !r.bit()? {
        return Err(Error::Format("Vorbis: setup header framing bit missing".into()));
    }
    Ok((w.finish(), Modes { blockflag, mode_bits: ilog(mode_count - 1) }))
}

// ----------------------------------------------------------------------------
// Ogg pages
// ----------------------------------------------------------------------------

fn ogg_crc_table() -> &'static [u32; 256] {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 { (r << 1) ^ 0x04c1_1db7 } else { r << 1 };
            }
            *e = r;
        }
        t
    })
}

fn ogg_crc(data: &[u8]) -> u32 {
    let t = ogg_crc_table();
    let mut crc = 0u32;
    for &b in data {
        crc = (crc << 8) ^ t[((crc >> 24) as u8 ^ b) as usize];
    }
    crc
}

/// Writes packets into Ogg pages (one logical stream, serial 1).
struct OggWriter {
    out: Vec<u8>,
    pending: Vec<Vec<u8>>,
    pending_bytes: usize,
    seqno: u32,
    first: bool,
}

impl OggWriter {
    fn new() -> Self {
        OggWriter { out: Vec::new(), pending: Vec::new(), pending_bytes: 0, seqno: 0, first: true }
    }

    /// Queue a packet; pages are flushed by `flush`.
    fn packet(&mut self, p: Vec<u8>) {
        self.pending_bytes += p.len();
        self.pending.push(p);
    }

    fn flush(&mut self, granule: u64, last: bool) {
        if self.pending.is_empty() && !last {
            return;
        }
        // Lacing: a packet of n bytes is n/255 values of 255 and one of n%255.
        let mut lacing = Vec::new();
        let mut payload = Vec::with_capacity(self.pending_bytes);
        for p in self.pending.drain(..) {
            let mut n = p.len();
            while n >= 255 {
                lacing.push(255u8);
                n -= 255;
            }
            lacing.push(n as u8);
            payload.extend_from_slice(&p);
        }
        self.pending_bytes = 0;
        // A page carries at most 255 lacing values: split when needed.
        let mut start = 0;
        let mut payload_pos = 0;
        while start < lacing.len() || (start == 0 && lacing.is_empty()) {
            let end = (start + 255).min(lacing.len());
            let seg = &lacing[start..end];
            let bytes: usize = seg.iter().map(|&l| l as usize).sum();
            let is_last_page = end == lacing.len();
            let mut page = Vec::with_capacity(27 + seg.len() + bytes);
            page.extend_from_slice(b"OggS");
            page.push(0);
            let continued = start > 0;
            page.push((continued as u8) | ((self.first as u8) << 1) | (((last && is_last_page) as u8) << 2));
            let g = if is_last_page { granule } else { u64::MAX };
            page.extend_from_slice(&g.to_le_bytes());
            page.extend_from_slice(&1u32.to_le_bytes());
            page.extend_from_slice(&self.seqno.to_le_bytes());
            page.extend_from_slice(&0u32.to_le_bytes());
            page.push(seg.len() as u8);
            page.extend_from_slice(seg);
            page.extend_from_slice(&payload[payload_pos..payload_pos + bytes]);
            let crc = ogg_crc(&page);
            page[22..26].copy_from_slice(&crc.to_le_bytes());
            self.out.extend_from_slice(&page);
            self.seqno += 1;
            self.first = false;
            payload_pos += bytes;
            start = end;
            if lacing.is_empty() {
                break;
            }
        }
    }

    fn finish(self) -> Vec<u8> {
        self.out
    }
}

/// Packets of a single-stream Ogg file.
fn ogg_packets(bytes: &[u8]) -> Result<(Vec<Vec<u8>>, u64)> {
    let mut packets = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    let mut pos = 0;
    let mut last_granule = 0u64;
    while pos + 27 <= bytes.len() {
        if &bytes[pos..pos + 4] != b"OggS" {
            return Err(Error::Format("Ogg: page sync lost".into()));
        }
        let granule = u64::from_le_bytes(bytes[pos + 6..pos + 14].try_into().unwrap());
        let nseg = bytes[pos + 26] as usize;
        let lacing = bytes.get(pos + 27..pos + 27 + nseg).ok_or_else(|| Error::Format("Ogg: truncated page".into()))?;
        let mut p = pos + 27 + nseg;
        for &l in lacing {
            let l = l as usize;
            let seg = bytes.get(p..p + l).ok_or_else(|| Error::Format("Ogg: truncated page".into()))?;
            current.extend_from_slice(seg);
            p += l;
            if l < 255 {
                packets.push(std::mem::take(&mut current));
            }
        }
        if granule != u64::MAX {
            last_granule = granule;
        }
        pos = p;
    }
    Ok((packets, last_granule))
}

// ----------------------------------------------------------------------------
// wem → ogg
// ----------------------------------------------------------------------------

/// What a Wwise Vorbis file says about itself.
#[derive(Clone, Debug, Default)]
pub struct WemInfo {
    pub channels: u32,
    pub sample_rate: u32,
    pub frames: u32,
    pub avg_bytes_per_sec: u32,
    pub blocksizes: [u8; 2],
    pub seek_table_size: u32,
    pub setup_hash: u32,
    pub packets: usize,
    pub codec: &'static str,
}

impl WemInfo {
    pub fn duration(&self) -> f64 {
        if self.sample_rate == 0 { 0.0 } else { self.frames as f64 / self.sample_rate as f64 }
    }
}

struct Riff<'a> {
    fmt: &'a [u8],
    data: &'a [u8],
}

fn riff(bytes: &[u8]) -> Result<Riff<'_>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(Error::Format("not a RIFF WAVE file".into()));
    }
    let riff_size = (u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8).min(bytes.len());
    let mut pos = 12;
    let mut fmt = None;
    let mut data = None;
    while pos + 8 <= riff_size {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = bytes.get(pos + 8..pos + 8 + size).ok_or_else(|| Error::Format("RIFF: chunk truncated".into()))?;
        match id {
            b"fmt " => fmt = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    Ok(Riff { fmt: fmt.ok_or_else(|| Error::Format("RIFF: no fmt chunk".into()))?, data: data.ok_or_else(|| Error::Format("RIFF: no data chunk".into()))? })
}

/// Header facts of a `.wem` (Vorbis or not).
pub fn info(bytes: &[u8]) -> Result<WemInfo> {
    let r = riff(bytes)?;
    let f = r.fmt;
    if f.len() < 16 {
        return Err(Error::Format("fmt chunk too short".into()));
    }
    let u16 = |o: usize| u16::from_le_bytes([f[o], f[o + 1]]);
    let u32 = |o: usize| u32::from_le_bytes(f[o..o + 4].try_into().unwrap());
    let tag = u16(0);
    let mut i = WemInfo { channels: u16(2) as u32, sample_rate: u32(4), avg_bytes_per_sec: u32(8), ..Default::default() };
    i.codec = match tag {
        0xFFFF => "Wwise Vorbis",
        0x0001 => "PCM",
        0x0002 => "Wwise ADPCM",
        0xFFFE => "PCM (extensible)",
        0x3039 => "Wwise Opus",
        0x3040 | 0x3041 => "Wwise Opus",
        _ => "unknown",
    };
    if tag == 0xFFFF && f.len() >= 0x42 {
        i.frames = u32(0x18);
        i.seek_table_size = u32(0x28);
        i.setup_hash = u32(0x3C);
        i.blocksizes = [f[0x40], f[0x41]];
        let mut pos = i.seek_table_size as usize;
        let mut n = 0;
        while pos + 2 <= r.data.len() {
            let size = u16::from_le_bytes([r.data[pos], r.data[pos + 1]]) as usize;
            pos += 2 + size;
            n += 1;
        }
        i.packets = n;
    } else if tag == 0x0001 || tag == 0xFFFE {
        let block_align = u16(12).max(1) as u32;
        i.frames = (r.data.len() as u32) / block_align;
    }
    Ok(i)
}

/// Convert a Wwise Vorbis `.wem` into a standard Ogg Vorbis file.
pub fn wem_to_ogg(bytes: &[u8]) -> Result<(Vec<u8>, WemInfo)> {
    let info = info(bytes)?;
    let r = riff(bytes)?;
    let f = r.fmt;
    if info.codec != "Wwise Vorbis" {
        return Err(Error::Format(format!("{} audio: only Wwise Vorbis converts to Ogg", info.codec)));
    }
    if f.len() < 0x42 {
        return Err(Error::Format("Wwise Vorbis with a separate vorb chunk (pre-2012) is not supported".into()));
    }
    let channels = info.channels;
    let data = r.data;
    let seek = info.seek_table_size as usize;
    let vorbis_data_offset = u32::from_le_bytes(f[0x2C..0x30].try_into().unwrap()) as usize;
    // Setup packet.
    if seek + 2 > data.len() {
        return Err(Error::Format("Wwise Vorbis: no setup packet".into()));
    }
    let setup_size = u16::from_le_bytes([data[seek], data[seek + 1]]) as usize;
    let setup = data.get(seek + 2..seek + 2 + setup_size).ok_or_else(|| Error::Format("Wwise Vorbis: setup packet truncated".into()))?;
    if seek + 2 + setup_size != vorbis_data_offset {
        return Err(Error::Format("Wwise Vorbis: first audio packet does not follow the setup packet".into()));
    }
    let mut ogg = OggWriter::new();
    // Identification.
    {
        let mut w = BitWriter::default();
        w.bits(1, 8);
        w.bytes(b"vorbis");
        w.bits(0, 32);
        w.bits(channels, 8);
        w.bits(info.sample_rate, 32);
        w.bits(0, 32);
        w.bits(info.avg_bytes_per_sec.wrapping_mul(8), 32);
        w.bits(0, 32);
        w.bits(info.blocksizes[0] as u32, 4);
        w.bits(info.blocksizes[1] as u32, 4);
        w.bit(true);
        ogg.packet(w.finish());
        ogg.flush(0, false);
    }
    // Comment.
    {
        let mut w = BitWriter::default();
        w.bits(3, 8);
        w.bytes(b"vorbis");
        let vendor = b"tempest-assets (Wwise Vorbis)";
        w.bits(vendor.len() as u32, 32);
        w.bytes(vendor);
        w.bits(0, 32);
        w.bit(true);
        ogg.packet(w.finish());
        ogg.flush(0, false);
    }
    // Setup.
    let modes = {
        let mut w = BitWriter::default();
        w.bits(5, 8);
        w.bytes(b"vorbis");
        let modes = unpack_setup(setup, channels, &mut w)?;
        ogg.packet(w.finish());
        ogg.flush(0, false);
        modes
    };
    // Audio packets: mod packets get their type bit and window flags back.
    let mut packets: Vec<&[u8]> = Vec::new();
    let mut pos = vorbis_data_offset;
    while pos + 2 <= data.len() {
        let size = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        let p = data.get(pos + 2..pos + 2 + size).ok_or_else(|| Error::Format("Wwise Vorbis: audio packet truncated".into()))?;
        packets.push(p);
        pos += 2 + size;
    }
    let blocksize = |flag: bool| 1u64 << info.blocksizes[flag as usize];
    let mode_of = |p: &[u8]| -> Result<usize> {
        let mut r = BitReader::new(p);
        Ok(r.bits(modes.mode_bits)? as usize)
    };
    let mut granule = 0u64;
    let mut prev_flag: Option<bool> = None;
    let mut page_bytes = 0usize;
    for (k, p) in packets.iter().enumerate() {
        if p.is_empty() {
            continue;
        }
        let mode = mode_of(p)?;
        let flag = *modes.blockflag.get(mode).ok_or_else(|| Error::Format("Wwise Vorbis: audio packet mode out of range".into()))?;
        let mut w = BitWriter::default();
        w.bit(false);
        w.bits(mode as u32, modes.mode_bits);
        if flag {
            let next_flag = packets.get(k + 1).filter(|n| !n.is_empty()).map(|n| modes.blockflag[mode_of(n).unwrap_or(0).min(modes.blockflag.len() - 1)]).unwrap_or(false);
            w.bit(prev_flag.unwrap_or(false));
            w.bit(next_flag);
        }
        let mut r = BitReader::new(p);
        r.bits(modes.mode_bits)?;
        while r.position() < r.total_bits() {
            w.bit(r.bit()?);
        }
        if let Some(pf) = prev_flag {
            granule += (blocksize(pf) + blocksize(flag)) / 4;
        }
        prev_flag = Some(flag);
        let out = w.finish();
        page_bytes += out.len();
        ogg.packet(out);
        let last = k + 1 == packets.len();
        if page_bytes >= 4096 || last {
            let g = if last { granule.min(info.frames as u64) } else { granule };
            ogg.flush(g, last);
            page_bytes = 0;
        }
    }
    Ok((ogg.finish(), info))
}

// ----------------------------------------------------------------------------
// ogg → wem
// ----------------------------------------------------------------------------

/// Decoder allocation sizes the game's own files carry per setup packet
/// (`dwDecodeAllocSize`, `dwDecodeX64AllocSize`), by setup hash; an unknown
/// setup gets the largest observed pair.
const KNOWN_ALLOC: &[(u32, u32, u32)] = &[
    (0x3fc5e0e6, 19488, 19984),
    (0x7fa9dd1f, 15920, 16360),
    (0xc574e4b8, 18892, 19332),
    (0x5921d930, 20904, 21400),
    (0x34123e25, 18240, 18736),
    (0x46ffffe1, 18436, 18916),
];

fn channel_config(channels: u32) -> u32 {
    let mask = match channels {
        1 => 0x4,
        2 => 0x3,
        4 => 0x603,
        6 => 0x60F,
        8 => 0x63F,
        _ => (1u32 << channels) - 1,
    };
    (mask << 12) | (1 << 8) | channels
}

/// Convert a standard Ogg Vorbis file (encoded by the aoTuV flavour of
/// libvorbis, which this crate's encoder is) into a Wwise Vorbis `.wem`.
pub fn ogg_to_wem(ogg: &[u8]) -> Result<Vec<u8>> {
    let (packets, last_granule) = ogg_packets(ogg)?;
    if packets.len() < 4 {
        return Err(Error::Format("Ogg: fewer than four packets".into()));
    }
    let ident = &packets[0];
    if ident.len() < 30 || ident[0] != 1 || &ident[1..7] != b"vorbis" {
        return Err(Error::Format("Ogg: not a Vorbis stream".into()));
    }
    let channels = ident[11] as u32;
    let sample_rate = u32::from_le_bytes(ident[12..16].try_into().unwrap());
    let blocksizes = [ident[28] & 0xF, ident[28] >> 4];
    let setup = &packets[2];
    if setup.len() < 7 || setup[0] != 5 {
        return Err(Error::Format("Ogg: third packet is not the setup header".into()));
    }
    let (packed_setup, modes) = pack_setup(&setup[7..], channels)?;
    // Audio packets → mod packets.
    let mut audio: Vec<Vec<u8>> = Vec::with_capacity(packets.len() - 3);
    let mut rendered = 0u64;
    let mut prev_flag: Option<bool> = None;
    for p in &packets[3..] {
        if p.is_empty() {
            continue;
        }
        let mut r = BitReader::new(p);
        if r.bit()? {
            return Err(Error::Format("Ogg: header packet among the audio packets".into()));
        }
        let mode = r.bits(modes.mode_bits)? as usize;
        let flag = *modes.blockflag.get(mode).ok_or_else(|| Error::Format("Ogg: packet mode out of range".into()))?;
        if flag {
            r.bit()?;
            r.bit()?;
        }
        let mut w = BitWriter::default();
        w.bits(mode as u32, modes.mode_bits);
        while r.position() < r.total_bits() {
            w.bit(r.bit()?);
        }
        if let Some(pf) = prev_flag {
            rendered += ((1u64 << blocksizes[pf as usize]) + (1u64 << blocksizes[flag as usize])) / 4;
        }
        prev_flag = Some(flag);
        let out = w.finish();
        if out.len() > u16::MAX as usize {
            return Err(Error::Format("Ogg: packet too large for Wwise".into()));
        }
        audio.push(out);
    }
    let frames = if last_granule == 0 || last_granule > rendered { rendered } else { last_granule } as u32;
    let excess = rendered.saturating_sub(frames as u64).min(u16::MAX as u64) as u32;
    // Data chunk: setup packet then audio packets, each `u16 size` + bytes.
    let mut data = Vec::new();
    data.extend_from_slice(&(packed_setup.len() as u16).to_le_bytes());
    data.extend_from_slice(&packed_setup);
    let first_audio = data.len() as u32;
    let mut max_packet = 0usize;
    for p in &audio {
        data.extend_from_slice(&(p.len() as u16).to_le_bytes());
        data.extend_from_slice(p);
        max_packet = max_packet.max(p.len());
    }
    let hash = fnv1_32(&packed_setup);
    let (alloc, alloc64) = KNOWN_ALLOC.iter().find(|k| k.0 == hash).map(|k| (k.1, k.2)).unwrap_or_else(|| KNOWN_ALLOC.iter().map(|k| (k.1, k.2)).max().unwrap());
    let avg = if frames > 0 { ((data.len() as u64 * sample_rate as u64) / frames as u64) as u32 } else { 0 };
    let mut fmt = vec![0u8; 0x42];
    let put16 = |f: &mut [u8], o: usize, v: u16| f[o..o + 2].copy_from_slice(&v.to_le_bytes());
    let put32 = |f: &mut [u8], o: usize, v: u32| f[o..o + 4].copy_from_slice(&v.to_le_bytes());
    put16(&mut fmt, 0x00, 0xFFFF);
    put16(&mut fmt, 0x02, channels as u16);
    put32(&mut fmt, 0x04, sample_rate);
    put32(&mut fmt, 0x08, avg);
    put16(&mut fmt, 0x0C, 0);
    put16(&mut fmt, 0x0E, 0);
    put16(&mut fmt, 0x10, 0x30);
    put16(&mut fmt, 0x12, 0);
    put32(&mut fmt, 0x14, channel_config(channels));
    put32(&mut fmt, 0x18, frames);
    put32(&mut fmt, 0x1C, first_audio);
    put32(&mut fmt, 0x20, data.len() as u32);
    put32(&mut fmt, 0x24, excess << 16);
    put32(&mut fmt, 0x28, 0);
    put32(&mut fmt, 0x2C, first_audio);
    put16(&mut fmt, 0x30, max_packet as u16);
    put16(&mut fmt, 0x32, excess as u16);
    put32(&mut fmt, 0x34, alloc);
    put32(&mut fmt, 0x38, alloc64);
    put32(&mut fmt, 0x3C, hash);
    fmt[0x40] = blocksizes[0];
    fmt[0x41] = blocksizes[1];
    let mut out = Vec::with_capacity(12 + 8 + fmt.len() + 8 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((4 + 8 + fmt.len() + 8 + data.len()) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
    out.extend_from_slice(&fmt);
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_loads() {
        let lib = Library::get();
        assert_eq!(lib.len(), 598);
        // Every book re-serialises to a form that parses back to itself.
        for (i, b) in lib.books.iter().enumerate() {
            let mut w = BitWriter::default();
            b.write_full(&mut w);
            let bytes = w.finish();
            let mut r = BitReader::new(&bytes);
            let back = Codebook::parse_full(&mut r).unwrap_or_else(|e| panic!("book {i}: {e}"));
            assert_eq!(&back, b, "book {i}");
            // The library holds a few duplicates: any id with the same book will do.
            assert_eq!(lib.books[lib.find(&back).unwrap()], *b);
        }
    }

    #[test]
    fn bit_io_round_trips() {
        let mut w = BitWriter::default();
        w.bits(0b101, 3);
        w.bits(0x564342, 24);
        w.bit(true);
        w.bits(1234, 14);
        let b = w.finish();
        let mut r = BitReader::new(&b);
        assert_eq!(r.bits(3).unwrap(), 0b101);
        assert_eq!(r.bits(24).unwrap(), 0x564342);
        assert!(r.bit().unwrap());
        assert_eq!(r.bits(14).unwrap(), 1234);
    }

}
