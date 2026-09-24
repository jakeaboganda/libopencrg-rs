//! Reader and evaluator for [ASAM OpenCRG](https://www.asam.net/standards/detail/opencrg/)
//! road surface files.
//!
//! [`CrgGrid`] loads a `.crg` file, ASCII or binary, and answers queries in grid
//! coordinates [`Uv`], with u along the reference line and v to its left, or in global
//! coordinates [`Xy`]:
//!
//! ```
//! use opencrg::{CrgGrid, Uv, Xy};
//!
//! // Three cross sections 1 m apart, each with three long sections 0.5 m apart from
//! // v = -0.5 m to 0.5 m, and a 10 cm bump in the middle.
//! let crg = b"$ROAD_CRG
//! REFERENCE_LINE_INCREMENT = 1.0
//! LONG_SECTION_V_RIGHT = -0.5
//! LONG_SECTION_V_INCREMENT = 0.5
//! $
//! $KD_Definition
//! #:LRFI
//! D:long section 1,m
//! D:long section 2,m
//! D:long section 3,m
//! $
//! $$$$
//!  0.0000000 0.0000000 0.0000000
//!  0.0000000 0.1000000 0.0000000
//!  0.0000000 0.0000000 0.0000000
//! ";
//! let grid = CrgGrid::from_bytes(crg)?;
//!
//! let z = grid.elevation_at_uv(Uv { u: 0.5, v: 0.0 }).unwrap();
//! assert!((z - 0.05).abs() < 1e-6);
//!
//! // Without modifiers the reference line starts at the origin, heading along x.
//! let xy = grid.xy_from_uv(Uv { u: 1.0, v: 0.25 });
//! assert_eq!(xy, Xy { x: 1.0, y: 0.25 });
//! assert_eq!(grid.uv_from_xy(xy), Some(Uv { u: 1.0, v: 0.25 }));
//! # Ok::<(), opencrg::Error>(())
//! ```
//!
//! Results follow the ASAM OpenCRG 2.0 C-API, including its defaults and border handling.
//! The README lists where the crate deliberately differs.

mod error;
mod eval;
mod grid;
mod parse;
mod refline;
mod types;

pub use error::Error;
pub use grid::CrgGrid;
pub use types::{BorderMode, GridSample, Heading, LoadOptions, Normal, Uv, Xy};

// The README promises that one grid can serve many threads.
const _: () = {
    const fn shareable<T: Send + Sync>() {}
    shareable::<CrgGrid>();
};

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
