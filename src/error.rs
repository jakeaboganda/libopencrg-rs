use std::{fmt, io};

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
    /// Reading a file failed. `path` is the file as named by the caller or an include.
    Io {
        path: String,
        kind: io::ErrorKind,
        message: String,
    },
    /// An included file failed to load; `error` says why.
    Include { path: String, error: Box<Error> },
    /// Includes are nested more than eight levels deep.
    IncludeTooDeep,
    /// A file includes itself, directly or through other files.
    IncludeCycle(String),
}

impl Error {
    pub(crate) fn io(path: impl Into<String>, error: &io::Error) -> Self {
        Error::Io {
            path: path.into(),
            kind: error.kind(),
            message: error.to_string(),
        }
    }
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
            Error::Io { path, message, .. } => write!(f, "{path}: {message}"),
            Error::Include { path, error } => write!(f, "in {path}: {error}"),
            Error::IncludeTooDeep => f.write_str("includes are nested more than 8 levels deep"),
            Error::IncludeCycle(path) => write!(f, "{path} includes itself"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Include { error, .. } => Some(error.as_ref()),
            _ => None,
        }
    }
}
