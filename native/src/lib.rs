//! The navigation core of the evil-doom planner (evildoom.nav uses it):
//!
//! - `mesh`: the navmesh, one convex poly per subsector, joined by portals.
//! - `nav`: which heights let the player across where, flood fill, path-finding with
//!   smoothing and fitting the player's box along the path.
//! - `plan`: the search for a route through triggers and keys to a goal.
//! - `python`: the classes Python uses (`NativeMesh`, `Nav`) and how values cross over.

mod geom;
mod map;
mod mesh;
mod nav;
mod plan;
mod python;
mod stats;

use pyo3::prelude::*;

#[pymodule]
fn evildoom_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<python::NativeMesh>()?;
    m.add_class::<python::Nav>()?;
    Ok(())
}
