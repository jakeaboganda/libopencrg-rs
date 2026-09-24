//! Reader and evaluator for [ASAM OpenCRG](https://www.asam.net/standards/detail/opencrg/)
//! road surface files.
//!
//! Results follow the ASAM OpenCRG C-API. See `docs/design.md` in the repository for the
//! design and its deliberate differences from the C-API.

mod error;
mod eval;
mod grid;
mod parse;
mod types;

pub use error::Error;
pub use grid::CrgGrid;
pub use types::{BorderMode, GridSample, LoadOptions, Uv, Xy};
