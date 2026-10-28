//! Typed view of the objects inside Paladins packages.
//!
//! * [`reflect`] loads the game's script packages and answers class /
//!   property / struct / enum questions (the `UClass` tree the engine would
//!   have in memory).
//! * [`value`] decodes tagged properties (`FPropertyTag` lists) into typed
//!   values against that reflection and encodes them back byte-for-byte.
//! * [`object`] parses one export into header + properties + native tail,
//!   and writes it back.
//! * [`dump`] renders objects as text; [`verify`] round-trips whole
//!   directories and measures how much of every export is understood.

pub mod actors;
pub mod anim;
pub mod anim_ops;
pub mod basestore;
pub mod batch;
pub mod copies;
pub mod bulk;
pub mod dds;
pub mod delta;
pub mod dump;
pub mod edit;
pub mod json;
pub mod level;
pub mod material;
pub mod material_ops;
pub mod media;
pub mod mergedindex;
pub mod mesh;
pub mod mesh_import;
pub mod fbx;
pub mod wwise;
pub mod audio;
pub mod particle;
pub mod mcts;
pub mod swf;
pub mod mesh_fbx;
pub mod mesh_ops;
pub mod misc;
pub mod object;
pub mod reflect;
pub mod serve;
pub mod texcache;
pub mod texture;
pub mod texture_ops;
pub mod twrk;
pub mod value;
pub mod verify;
pub mod visit;

pub use tempest_unrealscript as unrealscript;
pub use tempest_upk as upk;
