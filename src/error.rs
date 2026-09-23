use std::fmt;

/// Reason a CRG file could not be loaded.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// A header line could not be decoded. `line` is 1-based.
    Syntax { line: usize, reason: &'static str },
    /// The file uses a feature this crate does not implement.
    Unsupported(&'static str),
    /// A header value that the file's layout depends on is absent.
    Missing(&'static str),
    /// Header values contradict each other or the data.
    Invalid(&'static str),
    /// The file has no `$$$$` data section.
    NoData,
    /// The binary payload is shorter than the header declares.
    Truncated { expected: usize, actual: usize },
    /// The declared grid does not fit in addressable memory.
    TooLarge,
    /// The file has a `$ROAD_CRG_FILE` section, which needs an include loader.
    IncludeUnsupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Syntax { line, reason } => write!(f, "line {line}: {reason}"),
            Error::Unsupported(feature) => write!(f, "unsupported feature: {feature}"),
            Error::Missing(what) => write!(f, "missing {what}"),
            Error::Invalid(reason) => write!(f, "invalid file: {reason}"),
            Error::NoData => f.write_str("no $$$$ data section"),
            Error::Truncated { expected, actual } => {
                write!(
                    f,
                    "data section has {actual} bytes, header declares {expected}"
                )
            }
            Error::TooLarge => f.write_str("declared grid exceeds addressable memory"),
            Error::IncludeUnsupported => {
                f.write_str("$ROAD_CRG_FILE include needs an include loader")
            }
        }
    }
}

impl std::error::Error for Error {}
