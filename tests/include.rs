//! `$ROAD_CRG_FILE` includes through `from_bytes_with` and `from_path`. The precedence
//! cases for one include level were checked against the C-API; deeper nesting only works in
//! this crate.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use opencrg::{CrgGrid, Error, LoadOptions, Uv};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crg-txt")
}

fn straight() -> String {
    std::fs::read_to_string(fixtures().join("handmade_straight.crg")).unwrap()
}

fn include(name: &str) -> String {
    format!("$ROAD_CRG_FILE\n {name}\n$!\n")
}

fn opts(lines: &str) -> String {
    format!("$ROAD_CRG_OPTS\n{lines}\n$!\n")
}

/// Loads `top` with includes served from `files`.
fn load(top: &str, files: &[(&str, String)]) -> Result<CrgGrid, Error> {
    let files: HashMap<&str, &String> = files.iter().map(|(name, text)| (*name, text)).collect();
    CrgGrid::from_bytes_with(top.as_bytes(), &LoadOptions::default(), |name| {
        files
            .get(name)
            .map(|text| text.as_bytes().to_vec())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    })
}

/// Whether a query beyond the left edge is refused, which `BORDER_MODE_V = 0` does.
fn refuses_v(grid: &CrgGrid) -> bool {
    grid.elevation_at_uv(Uv { u: 5.0, v: 9.0 }).is_none()
}

#[test]
fn least_nested_options_block_wins() {
    let inner = format!("{}{}", opts("BORDER_MODE_V = 0"), straight());
    let files = [("inner.crg", inner)];
    let keep = opts("BORDER_MODE_V = 2");

    assert!(refuses_v(&load(&include("inner.crg"), &files).unwrap()));
    let before = format!("{keep}{}", include("inner.crg"));
    assert!(!refuses_v(&load(&before, &files).unwrap()));
    let after = format!("{}{keep}", include("inner.crg"));
    assert!(!refuses_v(&load(&after, &files).unwrap()));

    // Three levels, where the C-API fails: the middle file's block beats the deepest one.
    let deep = format!("{}{}", opts("BORDER_MODE_V = 2"), straight());
    let middle = format!("{}{}", include("deep.crg"), opts("BORDER_MODE_V = 0"));
    let files = [("deep.crg", deep), ("middle.crg", middle)];
    assert!(refuses_v(&load(&include("middle.crg"), &files).unwrap()));
}

#[test]
fn blocks_in_an_included_file_add_up() {
    let blocks = format!(
        "{}{}",
        opts("BORDER_MODE_V = 0"),
        opts("BORDER_OFFSET_V = 1")
    );
    // In the top-level file the second block replaces the first.
    let top = CrgGrid::from_bytes(format!("{blocks}{}", straight()).as_bytes()).unwrap();
    assert!(!refuses_v(&top));
    let files = [("two.crg", format!("{blocks}{}", straight()))];
    assert!(refuses_v(&load(&include("two.crg"), &files).unwrap()));
}

#[test]
fn header_values_apply_in_reading_order() {
    let inner = format!("$ROAD_CRG_MODS\n$!\n{}", straight());
    let files = [("inner.crg", inner)];
    let start_x = "$ROAD_CRG\nREFERENCE_LINE_START_X = 100\n$!\n";
    let x = |top: String| {
        load(&top, &files)
            .unwrap()
            .xy_from_uv(Uv { u: 0.0, v: 0.0 })
            .x
    };
    assert_eq!(x(format!("{start_x}{}", include("inner.crg"))), 0.0);
    assert_eq!(x(format!("{}{start_x}", include("inner.crg"))), 100.0);
}

#[test]
fn names_span_lines_up_to_blanks_and_comments() {
    let top = "$ROAD_CRG_FILE\n* comment\n  in\n  ner ! comment\n  .crg\n$!\n";
    assert!(load(top, &[("inner.crg", straight())]).is_ok());
}

#[test]
fn include_errors() {
    let top = include("inner.crg");
    assert_eq!(
        CrgGrid::from_bytes(top.as_bytes()).unwrap_err(),
        Error::IncludeUnsupported
    );
    assert!(matches!(
        load(&top, &[]).unwrap_err(),
        Error::Io { path, kind: io::ErrorKind::NotFound, .. } if path == "inner.crg"
    ));
    let bad = format!("{}{}", opts("BORDER_MODE_U = 7"), straight());
    match load(&top, &[("inner.crg", bad)]).unwrap_err() {
        Error::Include { path, error } => {
            assert_eq!(path, "inner.crg");
            assert!(matches!(*error, Error::Syntax { line: 2, .. }), "{error:?}");
        }
        other => panic!("{other:?}"),
    }
    let looping = [("inner.crg", include("inner.crg"))];
    assert_eq!(load(&top, &looping).unwrap_err(), Error::IncludeTooDeep);
    assert!(matches!(
        load("$ROAD_CRG_FILE\n$!\n", &[]).unwrap_err(),
        Error::Missing(_)
    ));
    let twice = format!("{top}{}", straight());
    assert!(matches!(
        load(&twice, &[("inner.crg", straight())]).unwrap_err(),
        Error::Invalid(_)
    ));
}

#[test]
fn from_path_matches_loading_the_included_file() {
    let direct = CrgGrid::from_path(fixtures().join("handmade_curved_banked_sloped.crg")).unwrap();
    let included = CrgGrid::from_path(fixtures().join("fileref_opts.crg")).unwrap();
    for (u, v) in [(3.3, 0.4), (17.9, -1.2), (-2.0, 2.5)] {
        let at = Uv { u, v };
        assert_eq!(direct.elevation_at_uv(at), included.elevation_at_uv(at));
        assert_eq!(direct.xy_from_uv(at), included.xy_from_uv(at));
    }
    // Names a file that does not exist.
    match CrgGrid::from_path(fixtures().join("testOptionBorderMode.crg")).unwrap_err() {
        Error::Io { path, kind, .. } => {
            assert_eq!(kind, io::ErrorKind::NotFound);
            assert!(path.ends_with("handmade_sloped_opt.crg"), "{path}");
        }
        other => panic!("{other:?}"),
    }
}

/// A scratch directory for files that name other files by path.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("include-{test}"));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn from_path_expands_variables_and_detects_cycles() {
    let scratch = Scratch::new("paths");
    // Cargo sets CARGO_MANIFEST_DIR for test processes too.
    let name = "$CARGO_MANIFEST_DIR/tests/fixtures/crg-txt/handmade_straight.crg";
    let top = scratch.write("variable.crg", &include(name));
    assert!(CrgGrid::from_path(&top).is_ok());

    let unset = scratch.write("unset.crg", &include("$OPENCRG_UNSET_VARIABLE/x.crg"));
    assert!(matches!(
        CrgGrid::from_path(&unset).unwrap_err(),
        Error::Io {
            kind: io::ErrorKind::NotFound,
            ..
        }
    ));

    std::fs::create_dir_all(scratch.0.join("sub")).unwrap();
    scratch.write("a.crg", &include("sub/b.crg"));
    scratch.write("sub/b.crg", &include("../a.crg"));
    assert!(matches!(
        CrgGrid::from_path(scratch.0.join("a.crg")).unwrap_err(),
        Error::IncludeCycle(path) if path.ends_with("a.crg")
    ));
}
