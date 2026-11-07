//! Havok AI data stored by the game: the nav mesh tagfile in a
//! `HavokNavMeshActor`'s bulk data.
//!
//! * [`tagfile`]: Havok 2016 `TAG0` tagfiles, read, generic values, byte-identical write.
//! * [`navmesh`]: typed `hkaiNavMesh`, cluster graph, annotation library.
//! * [`tree`]: the query mediator's compressed AABB tree.

pub mod build;
pub mod edit;
pub mod navmesh;
pub mod studio;
pub mod tagfile;
pub mod tree;
pub mod view;

use tempest_upk::io::{Error, Result};

use crate::object::{Native, Object};

/// The tagfile bytes of a parsed `HavokNavMeshActor` (None when its bulk
/// data is empty, as on a few levels that carry an unused actor).
pub fn actor_tagfile(obj: &Object) -> Result<Option<Vec<u8>>> {
    let Native::HavokNavMesh(h) = &obj.native else { return Err(Error::Format("not a HavokNavMeshActor".into())) };
    if h.data.is_compressed() {
        return Err(Error::Format("compressed nav mesh bulk data is not supported".into()));
    }
    match &h.data.inline {
        Some(d) if Tagfile::stored_len(d).is_some() => Ok(Some(d.clone())),
        _ => Ok(None),
    }
}

/// Replace the actor's tagfile (stored raw, like the cooker does).
pub fn set_actor_tagfile(obj: &mut Object, bytes: &[u8]) -> Result<()> {
    let Native::HavokNavMesh(h) = &mut obj.native else { return Err(Error::Format("not a HavokNavMeshActor".into())) };
    h.data.set_inline(bytes, false, false);
    Ok(())
}

pub use tagfile::Tagfile;
