//! Reads a `.crg` file into header values and channel arrays, following `crgLoader.c` of the
//! ASAM OpenCRG C-API. Reference-line integration and modifiers happen later, in the grid.

mod data;
mod header;

use data::{Format, decode_records, split_line};
use header::c_atof;
pub(crate) use header::{Mods, NanMode, Options, Road};

use crate::Error;

/// Contents of one `.crg` file before any derived data is computed.
#[derive(Debug)]
pub(crate) struct Parsed {
    pub road: Road,
    /// The last `$ROAD_CRG_OPTS` block, if any. An empty block is `Some(default)`.
    pub options: Option<Options>,
    /// The last `$ROAD_CRG_MODS` block, if any. An empty block is `Some(default)`.
    pub mods: Option<Mods>,
    /// Long-section positions, ascending.
    pub v: Vec<f64>,
    /// `true` when `D:long section at v = ...` gave positions; `false` when numbered sections
    /// took positions from `LONG_SECTION_V_RIGHT` and `LONG_SECTION_V_INCREMENT`.
    pub v_from_positions: bool,
    /// Number of cross sections.
    pub nu: usize,
    /// Grid values, cross-section major: `z[iu * v.len() + iv]`.
    pub z: Vec<f32>,
    /// Heading per cross section. The first value is `REFERENCE_LINE_START_PHI`, not the file's.
    pub phi: Option<Vec<f64>>,
    pub bank: Option<Vec<f64>>,
    pub slope: Option<Vec<f64>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Comment,
    Road,
    Options,
    Mods,
    Mpro,
    DataDef,
    File,
}

#[derive(Clone, Copy)]
enum Column {
    Z,
    Phi,
    Bank,
    Slope,
    Ignored,
}

/// `D:long section` definitions in file order.
struct LongSection {
    position: f64,
    column: usize,
    by_position: bool,
}

#[derive(Default)]
struct DataDef {
    format: Option<Format>,
    columns: Vec<Column>,
    long_sections: Vec<LongSection>,
}

/// Deepest include level below the top-level file.
const MAX_INCLUDE_DEPTH: usize = 8;

/// Loads an included file. Receives the include names from the top-level file down to the
/// one to load, as written in each `$ROAD_CRG_FILE` section.
pub(crate) type Include<'a> = dyn FnMut(&[String]) -> Result<Vec<u8>, Error> + 'a;

pub(crate) fn parse(bytes: &[u8], include: &mut Include) -> Result<Parsed, Error> {
    let mut reader = Reader {
        road: Road::default(),
        options: None,
        mods: None,
        options_level: None,
        mods_level: None,
        def: DataDef::default(),
        data: None,
        chain: Vec::new(),
        include,
    };
    reader.read(bytes)?;
    let data = reader.data.ok_or(Error::NoData)?;
    Ok(Parsed {
        road: reader.road,
        options: reader.options,
        mods: reader.mods,
        v: data.v,
        v_from_positions: data.v_from_positions,
        nu: data.nu,
        z: data.z,
        phi: data.phi,
        bank: data.bank,
        slope: data.slope,
    })
}

/// The data section, decoded with the header values read before it.
struct Data {
    v: Vec<f64>,
    v_from_positions: bool,
    nu: usize,
    z: Vec<f32>,
    phi: Option<Vec<f64>>,
    bank: Option<Vec<f64>>,
    slope: Option<Vec<f64>>,
}

/// State shared by a file and the files it includes, which the C-API reads into one data
/// set.
struct Reader<'a, 'b> {
    road: Road,
    options: Option<Options>,
    mods: Option<Mods>,
    /// Include level of the file whose options or modifiers block is in effect.
    options_level: Option<usize>,
    mods_level: Option<usize>,
    def: DataDef,
    data: Option<Data>,
    /// Include names from the top-level file down to the file being read.
    chain: Vec<String>,
    include: &'a mut Include<'b>,
}

impl Reader<'_, '_> {
    /// Reads one file up to and including its data section.
    fn read(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let level = self.chain.len();
        let mut section = Section::None;
        let mut name = String::new();
        let mut pos = 0;
        let mut line_no = 0;

        loop {
            if pos >= bytes.len() {
                if section == Section::File {
                    self.load_include(&name)?;
                }
                return Ok(());
            }
            let (raw, next) = split_line(bytes, pos);
            pos = next;
            line_no += 1;
            let text = String::from_utf8_lossy(raw);
            let line = text.as_ref();
            let syntax = |reason| Error::Syntax {
                line: line_no,
                reason,
            };

            if section == Section::Comment {
                if line.starts_with('$') {
                    section = Section::None;
                }
                continue;
            }
            if is_comment(line) {
                continue;
            }
            let tag = line.trim_start_matches(' ');
            if section == Section::File {
                // Only a `$` in the first column ends the section, since a name may start
                // with an environment variable (crgLoader.c:662).
                if line.starts_with('$') {
                    self.load_include(&std::mem::take(&mut name))?;
                    section = Section::None;
                } else {
                    // A name may span lines; each contributes up to a blank or comment.
                    name.extend(tag.chars().take_while(|&c| c != ' ' && c != '!'));
                }
                continue;
            }
            if tag.starts_with('$') {
                if section != Section::None {
                    section = Section::None;
                    continue;
                }
                let tag = tag.to_ascii_uppercase();
                section = if tag.starts_with("$ROAD_CRG_MODS") {
                    if replaces(level, self.mods_level) {
                        self.mods = Some(Mods::default());
                        self.mods_level = Some(level);
                    }
                    Section::Mods
                } else if tag.starts_with("$ROAD_CRG_OPTS") {
                    if replaces(level, self.options_level) {
                        self.options = Some(Options::default());
                        self.options_level = Some(level);
                    }
                    Section::Options
                } else if tag.starts_with("$ROAD_CRG_FILE") {
                    Section::File
                } else if tag.starts_with("$ROAD_CRG_MPRO") {
                    Section::Mpro
                } else if tag.starts_with("$ROAD_CRG") {
                    Section::Road
                } else if tag.starts_with("$CT") {
                    Section::Comment
                } else if tag.starts_with("$KD_DEFINITION") {
                    Section::DataDef
                } else if tag.starts_with("$$$$") {
                    if self.data.is_some() {
                        return Err(Error::Invalid("more than one data section"));
                    }
                    self.data = Some(self.decode(&bytes[pos..])?);
                    return Ok(());
                } else {
                    Section::None
                };
                continue;
            }
            match section {
                Section::Road => self.road.apply(line).map_err(syntax)?,
                Section::Options if level == 0 || self.options_level == Some(level) => {
                    let options = self.options.as_mut().unwrap();
                    options.apply(line).map_err(syntax)?;
                }
                Section::Mods if level == 0 || self.mods_level == Some(level) => {
                    self.mods.as_mut().unwrap().apply(line).map_err(syntax)?;
                }
                Section::DataDef => self.def.apply(tag).map_err(|e| match e {
                    DefError::Syntax(reason) => syntax(reason),
                    DefError::Unsupported(feature) => Error::Unsupported(feature),
                })?,
                _ => {}
            }
        }
    }

    fn load_include(&mut self, name: &str) -> Result<(), Error> {
        if name.is_empty() {
            return Err(Error::Missing("file name in $ROAD_CRG_FILE"));
        }
        if self.chain.len() == MAX_INCLUDE_DEPTH {
            return Err(Error::IncludeTooDeep);
        }
        self.chain.push(name.to_owned());
        let bytes = (self.include)(&self.chain)?;
        let result = self.read(&bytes);
        self.chain.pop();
        result.map_err(|error| match error {
            // Report where the innermost failure happened.
            Error::Include { .. } | Error::Io { .. } | Error::IncludeCycle(_) => error,
            Error::IncludeTooDeep => error,
            error => Error::Include {
                path: name.to_owned(),
                error: Box::new(error),
            },
        })
    }

    fn decode(&mut self, bytes: &[u8]) -> Result<Data, Error> {
        let sections = std::mem::take(&mut self.def.long_sections);
        let (road, def) = (&self.road, &self.def);
        let format = def.format.ok_or(Error::Missing("#: data format"))?;
        let (v, v_from_positions, z_columns) = long_sections(road, sections)?;
        let nv = v.len();

        let binary_records = if format.ascii {
            0
        } else {
            let end_u = road
                .end_u
                .ok_or(Error::Missing("REFERENCE_LINE_END_U for binary data"))?;
            let steps =
                (end_u - road.start_u.unwrap_or(0.0)) / road.u_increment.unwrap_or(0.01) + 0.5;
            if !(steps >= 0.0 && steps.is_finite()) {
                return Err(Error::Invalid("reference line end_u is before start_u"));
            }
            (steps as usize).checked_add(1).ok_or(Error::TooLarge)?
        };
        let capacity = binary_records.checked_mul(nv).ok_or(Error::TooLarge)?;
        let has = |wanted: fn(&Column) -> bool| def.columns.iter().any(wanted);
        let mut z = Vec::with_capacity(capacity);
        let mut phi = has(|c| matches!(c, Column::Phi)).then(Vec::new);
        let mut bank = has(|c| matches!(c, Column::Bank)).then(Vec::new);
        let mut slope = has(|c| matches!(c, Column::Slope)).then(Vec::new);
        let start_phi = road.start_phi.unwrap_or(0.0);

        let nu = decode_records(bytes, format, def.columns.len(), binary_records, |record| {
            z.extend(z_columns.iter().map(|&c| record[c] as f32));
            for (column, &value) in def.columns.iter().zip(record) {
                match column {
                    Column::Phi => {
                        let phi = phi.as_mut().unwrap();
                        phi.push(if phi.is_empty() { start_phi } else { value });
                    }
                    Column::Bank => bank.as_mut().unwrap().push(value),
                    Column::Slope => slope.as_mut().unwrap().push(value),
                    Column::Z | Column::Ignored => {}
                }
            }
        })?;
        if nu == 0 {
            return Err(Error::Invalid("data section has no complete record"));
        }
        Ok(Data {
            v,
            v_from_positions,
            nu,
            z,
            phi,
            bank,
            slope,
        })
    }
}

/// Whether an options or modifiers block in a file at include `level` replaces the one
/// defined at `current`. The block of the least nested file counts; the top-level file's
/// last block replaces its earlier ones, while blocks in one included file add up
/// (crgLoader.c:1000-1024).
fn replaces(level: usize, current: Option<usize>) -> bool {
    level == 0 || current.is_none_or(|current| level < current)
}

/// Empty lines and lines whose first non-space character is `*`.
fn is_comment(line: &str) -> bool {
    line.is_empty() || line.trim_start_matches(' ').starts_with('*')
}

enum DefError {
    Syntax(&'static str),
    Unsupported(&'static str),
}

impl DataDef {
    /// Applies one `$KD_Definition` line with leading spaces removed.
    fn apply(&mut self, line: &str) -> Result<(), DefError> {
        let head = line.get(..2).unwrap_or("").to_ascii_uppercase();
        let body = &line[head.len()..];
        match head.as_str() {
            "#:" => self.format = Some(Format::from_code(body)),
            "D:" => self.define(body.split('!').next().unwrap())?,
            _ => {}
        }
        Ok(())
    }

    /// Registers one data column, as `decodeDefined` does. Channels other than long sections
    /// and reference-line data still occupy a column.
    fn define(&mut self, body: &str) -> Result<(), DefError> {
        use DefError::{Syntax, Unsupported};
        let lower = body.to_ascii_lowercase();
        let column = self.columns.len();

        let kind = if let Some(i) = lower.find("long section") {
            let rest = &body[i + "long section".len()..];
            let lrest = &lower[i + "long section".len()..];
            let (by_position, number) = match lrest.find("at v ") {
                Some(j) => {
                    let eq = lrest[j..]
                        .find('=')
                        .ok_or(Syntax("long section position needs `=`"))?;
                    (true, &rest[j + eq + 1..])
                }
                None => (false, rest),
            };
            let position = c_atof(number).ok_or(Syntax("long section needs a number"))?;
            let comma = number
                .find(',')
                .ok_or(Syntax("long section needs a unit"))?;
            if !number[comma + 1..].starts_with('m') {
                return Err(Syntax("long section unit must be m"));
            }
            self.long_sections.push(LongSection {
                position,
                column,
                by_position,
            });
            Column::Z
        } else if let Some(i) = lower.find("reference line") {
            let rest = &lower[i + "reference line".len()..];
            let (at, column, unit) = if rest.contains(['x', 'y', 'u']) {
                return Err(Unsupported("reference line x, y, or u data channels"));
            } else if let Some(at) = rest.find("phi") {
                (at, Column::Phi, "rad")
            } else if let Some(at) = rest.find("banking") {
                (at, Column::Bank, "m/m")
            } else if let Some(at) = rest.find("slope") {
                (at, Column::Slope, "m/m")
            } else {
                return Err(Syntax("unknown reference line channel"));
            };
            let comma = rest[at..]
                .find(',')
                .ok_or(Syntax("reference line channel needs a unit"))?;
            if !rest[at + comma..].contains(unit) {
                return Err(Syntax("wrong reference line channel unit"));
            }
            column
        } else {
            Column::Ignored
        };
        self.columns.push(kind);
        Ok(())
    }
}

/// Orders long sections by position and derives v, as `prepareFromPosDef` and
/// `prepareFromIndexDef` do. Returns v, whether v came from positions, and the data column
/// of each long section.
fn long_sections(
    road: &Road,
    mut sections: Vec<LongSection>,
) -> Result<(Vec<f64>, bool, Vec<usize>), Error> {
    if sections.len() < 2 {
        return Err(Error::Invalid("fewer than two long sections"));
    }
    let by_position = sections[0].by_position;
    if sections.iter().any(|s| s.by_position != by_position) {
        return Err(Error::Invalid(
            "long sections mix `at v =` positions and numbers",
        ));
    }
    sections.sort_by(|a, b| a.position.total_cmp(&b.position));
    let columns = sections.iter().map(|s| s.column).collect();

    let v = if by_position {
        let v: Vec<f64> = sections.iter().map(|s| s.position).collect();
        if v.windows(2).any(|w| w[1] - w[0] < 1e-6) {
            return Err(Error::Invalid("long section spacing below 1e-6 m"));
        }
        v
    } else {
        if sections
            .iter()
            .enumerate()
            .any(|(i, s)| s.position != (i + 1) as f64)
        {
            return Err(Error::Invalid("long section numbers must run 1, 2, 3, ..."));
        }
        let first = road.v_right.unwrap_or(0.0);
        let inc = road.v_increment.unwrap_or(0.01);
        (0..sections.len())
            .map(|i| first + i as f64 * inc)
            .collect()
    };
    Ok((v, by_position, columns))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BorderMode;
    use std::path::Path;

    fn parse_plain(bytes: &[u8]) -> Result<Parsed, Error> {
        parse(bytes, &mut |_: &[String]| Err(Error::IncludeUnsupported))
    }

    fn fixture(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn txt(name: &str) -> Result<Parsed, Error> {
        parse_plain(&fixture(&format!("crg-txt/{name}")))
    }

    #[test]
    fn minimalist_equals_commented_straight() {
        let full = txt("handmade_straight.crg").unwrap();
        let min = txt("handmade_straight_minimalist.crg").unwrap();
        assert_eq!((full.nu, full.v.len()), (23, 7));
        assert_eq!(full.v, [-1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5]);
        assert_eq!(min.v, [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0]);
        assert_eq!(full.z.len(), min.z.len());
        assert!(
            full.z
                .iter()
                .zip(&min.z)
                .all(|(a, b)| a == b || (a.is_nan() && b.is_nan()))
        );
        assert!(full.z[7 * 7].is_nan());
        assert_eq!(full.z[7 * 7 + 1], 0.0111111);
    }

    #[test]
    fn double_file_matches_single_file_grid() {
        let single = txt("handmade_straight.crg").unwrap();
        let double = txt("handmade_straight_double.crg").unwrap();
        assert_eq!((double.nu, double.v.len()), (single.nu, single.v.len()));
        let same = |a: &f32, b: &f32| (a - b).abs() < 1e-6 || (a.is_nan() && b.is_nan());
        assert!(single.z.iter().zip(&double.z).all(|(a, b)| same(a, b)));
    }

    #[test]
    fn positioned_sections_and_reference_channels() {
        let p = txt("handmade_curved_banked_sloped.crg").unwrap();
        assert!(p.v_from_positions);
        assert_eq!(p.v, [-1.5, -1.25, -1.0, 0.0, 1.0, 1.25, 1.5]);
        assert_eq!(p.nu, 23);
        let phi = p.phi.unwrap();
        assert_eq!(phi.len(), 23);
        assert_eq!(phi[0], 0.0);
        let bank = p.bank.unwrap();
        assert_eq!((bank[0], bank[1], bank[2]), (0.0, 0.0, 0.011));
        assert_eq!(p.slope.as_ref().unwrap().len(), 23);
        assert_eq!(p.z[..7], [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(p.z[7..14], [0.0, 0.0, 0.0, 0.0111111, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn options_block_is_read() {
        let p = txt("testOptionBorderMode.crg");
        assert_eq!(p.unwrap_err(), Error::IncludeUnsupported);
        let p = txt("handmade_sloped_opts.crg").unwrap();
        assert!(p.options.is_some());
    }

    #[test]
    fn every_text_fixture_parses_or_needs_includes() {
        for entry in
            std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crg-txt"))
                .unwrap()
        {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            match parse_plain(&std::fs::read(&path).unwrap()) {
                Ok(p) => assert_eq!(p.z.len(), p.nu * p.v.len(), "{name}"),
                Err(Error::IncludeUnsupported) => assert!(
                    name.starts_with("fileref") || name.starts_with("testOption"),
                    "{name}"
                ),
                Err(e) => panic!("{name}: {e}"),
            }
        }
    }

    #[test]
    fn belgian_block_binary() {
        let p = parse_plain(&fixture("crg-bin/belgian_block.crg")).unwrap();
        let steps =
            (p.road.end_u.unwrap() - p.road.start_u.unwrap_or(0.0)) / p.road.u_increment.unwrap();
        assert_eq!(p.nu, (steps + 0.5) as usize + 1);
        assert_eq!(p.z.len(), p.nu * p.v.len());
        assert_eq!(p.phi.as_ref().unwrap().len(), p.nu);
        assert!(p.options.is_some());
        assert_eq!(p.options.unwrap().border_mode_u, None);
    }

    #[test]
    fn border_mode_from_file_options() {
        let file = b"$ROAD_CRG\nREFERENCE_LINE_INCREMENT = 1.0\n$\n$ROAD_CRG_OPTS\nBORDER_MODE_V = 0\n$\n\
                     $KD_Definition\n#:LRFI\nD:long section 1,m\nD:long section 2,m\n$\n$$$$\n 1.0000000 2.0000000\n";
        let p = parse_plain(file).unwrap();
        assert_eq!(p.options.unwrap().border_mode_v, Some(BorderMode::None));
        assert_eq!(p.v, [0.0, 0.01]);
        assert_eq!(p.z, [1.0, 2.0]);
    }

    #[test]
    fn missing_separator_ends_section_like_the_c_api() {
        let file = b"$KD_Definition\n#:LRFI\nD:long section 1,m\nD:long section 2,m\n$$$$\n 1.0000000 2.0000000\n";
        assert_eq!(parse_plain(file).unwrap_err(), Error::NoData);
    }

    #[test]
    fn generated_binary_formats() {
        let header = |code: &str| {
            format!(
                "$ROAD_CRG\nREFERENCE_LINE_END_U = 2.0\nREFERENCE_LINE_INCREMENT = 1.0\n$\n$KD_Definition\n#:{code}\n\
                 D:reference line phi,rad\nD:long section at v = 1.0,m\nD:long section at v = -1.0,m\n$\n$$$$\n"
            )
            .into_bytes()
        };
        let rows: [[f64; 3]; 3] = [[9.0, 1.0, 2.0], [0.5, 3.0, 4.0], [0.25, 5.0, 6.0]];
        for code in ["KRBI", "KDBI", "LRBI", "LDBI"] {
            let format = Format::from_code(code);
            let mut file = header(code);
            for row in rows {
                let start = file.len();
                for x in row {
                    if format.double {
                        file.extend_from_slice(&x.to_be_bytes());
                    } else {
                        file.extend_from_slice(&(x as f32).to_be_bytes());
                    }
                }
                if format.long {
                    file.resize(start + 80, 0);
                }
            }
            let p = parse_plain(&file).unwrap();
            assert_eq!(p.v, [-1.0, 1.0], "{code}");
            assert_eq!(p.z, [2.0, 1.0, 4.0, 3.0, 6.0, 5.0], "{code}");
            assert_eq!(p.phi.unwrap(), [0.0, 0.5, 0.25], "{code}");
        }
    }

    /// Needs the large sample files; run `tools/fetch-large-fixtures.sh` first.
    #[test]
    #[ignore]
    fn large_binaries() {
        for name in [
            "country_road",
            "crg_local_curv_test_fail",
            "crg_local_curv_test_ok",
            "crg_refline_Hoki_HoeKi_Grafing",
        ] {
            let p = parse_plain(&fixture(&format!("large/{name}.crg"))).unwrap();
            assert_eq!(p.z.len(), p.nu * p.v.len(), "{name}");
            assert!(p.nu > 1000, "{name}");
        }
    }

    #[test]
    fn reference_line_xy_is_unsupported() {
        let file = b"$KD_Definition\n#:LRFI\nD:reference line x,m\nD:reference line y,m\n$\n$$$$\n";
        assert!(matches!(parse_plain(file), Err(Error::Unsupported(_))));
    }
}
