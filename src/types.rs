/// Position in grid coordinates in metres: `u` along the reference line, `v` to its left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Uv {
    pub u: f64,
    pub v: f64,
}

/// Position in the global Cartesian frame in metres.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Xy {
    pub x: f64,
    pub y: f64,
}

/// Grid elevation and its slopes, without reference-line height, slope, bank, or any shift
/// applied at load.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GridSample {
    /// Bilinear grid elevation in metres.
    pub z: f64,
    /// Slope along u within the evaluated cell; 0 where a border holds the value.
    pub dz_du: f64,
    /// Slope along v within the evaluated cell; 0 where a border holds the value.
    pub dz_dv: f64,
}

/// Settings the caller chooses at load. Each value that the file's `$ROAD_CRG_OPTS` block
/// sets overrides the one here.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct LoadOptions {
    /// Behaviour before the first and after the last cross section.
    pub border_mode_u: BorderMode,
    /// Behaviour beyond the outer long sections.
    pub border_mode_v: BorderMode,
    /// Elevation offset beyond the u border in metres. Replaces the elevation in `Zero` mode.
    pub border_offset_u: f64,
    /// Elevation offset beyond the v border in metres. Replaces the elevation in `Zero` mode.
    pub border_offset_v: f64,
}

impl Default for LoadOptions {
    /// The C-API defaults: `Keep` on both axes, no offsets.
    fn default() -> Self {
        Self {
            border_mode_u: BorderMode::Keep,
            border_mode_v: BorderMode::Keep,
            border_offset_u: 0.0,
            border_offset_v: 0.0,
        }
    }
}

/// What a query returns beyond the grid edge along one axis.
///
/// Matches the C-API's `dCrgBorderMode*` values and the `BORDER_MODE_U`/`BORDER_MODE_V`
/// file options, which use the numbers in parentheses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BorderMode {
    /// (0) No surface: queries return `None`.
    None,
    /// (1) Grid elevation is zero.
    Zero,
    /// (2) The value at the nearest edge continues outward.
    #[default]
    Keep,
    /// (3) The grid repeats.
    Repeat,
    /// (4) The grid repeats, mirrored at each edge.
    Reflect,
}

impl BorderMode {
    pub(crate) fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            0 => Self::None,
            1 => Self::Zero,
            2 => Self::Keep,
            3 => Self::Repeat,
            4 => Self::Reflect,
            _ => return None,
        })
    }
}
