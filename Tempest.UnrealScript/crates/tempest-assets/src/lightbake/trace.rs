//! Rays in batches: what the path-traced bake asks of a tracer. The CPU
//! tracer walks the scene's [`Bvh`] over the cores; a host with a GPU (the
//! native Studio) can supply one that walks the same tree in a compute
//! shader ([`Bvh::flat`] is its layout), given through a [`TracerFactory`].

use super::bvh::{Bvh, Hit, V3};

/// A ray `o + t·d` for `0 < t < tmax` (`d` unit).
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub o: V3,
    pub d: V3,
    pub tmax: f32,
}

pub trait Tracer: Send + Sync {
    /// What it runs on, for the report ("CPU", the GPU's name).
    fn name(&self) -> String;
    /// The nearest hit of each ray.
    fn closest(&self, rays: &[Ray]) -> Vec<Option<Hit>>;
    /// Whether anything blocks each ray.
    fn occluded(&self, rays: &[Ray]) -> Vec<bool>;
}

/// Makes a tracer over a scene's tree (`Err`: why there is none, e.g. no GPU).
pub type TracerFactory = dyn Fn(&Bvh) -> Result<Box<dyn Tracer>, String> + Send + Sync;

/// Rays per job of the CPU tracer.
const CHUNK: usize = 4096;

pub struct CpuTracer<'a>(pub &'a Bvh);

impl Tracer for CpuTracer<'_> {
    fn name(&self) -> String {
        "CPU".into()
    }

    fn closest(&self, rays: &[Ray]) -> Vec<Option<Hit>> {
        let chunks: Vec<&[Ray]> = rays.chunks(CHUNK).collect();
        tempest_unrealscript::par::map(&chunks, |c| c.iter().map(|r| self.0.closest(r.o, r.d, r.tmax)).collect::<Vec<_>>()).into_iter().flatten().collect()
    }

    fn occluded(&self, rays: &[Ray]) -> Vec<bool> {
        let chunks: Vec<&[Ray]> = rays.chunks(CHUNK).collect();
        tempest_unrealscript::par::map(&chunks, |c| c.iter().map(|r| self.0.occluded(r.o, r.d, r.tmax)).collect::<Vec<_>>()).into_iter().flatten().collect()
    }
}
