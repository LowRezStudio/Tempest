//! Audiokinetic Wwise, as Paladins ships it: `.pck` file packages holding
//! soundbanks (`.bnk`) and streamed media (`.wem`), Wwise Vorbis audio in
//! every one of them (bank version 120, Wwise 2019).
//!
//! * [`akpk`]: the package container (tables of banks, streams, externals).
//! * [`bnk`]: soundbank chunks, embedded media (`DIDX`/`DATA`), the object
//!   hierarchy (`HIRC`: events → actions → sounds → media ids).
//! * [`vorbis`]: Wwise Vorbis `.wem` ⇄ standard Ogg Vorbis.
//! * [`fnv`]: the FNV-1 hash Wwise names everything by.

pub mod akpk;
pub mod bnk;
pub mod fnv;
pub mod vorbis;
