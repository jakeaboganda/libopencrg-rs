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
