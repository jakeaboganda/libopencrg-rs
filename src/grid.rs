//! Builds a [`CrgGrid`] from parsed file contents, following `crgLoaderPrepareData` and
//! `crgDataSetModifiersApply` of the C-API.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::{env, fs, io};

use crate::eval::Borders;
use crate::parse::{self, Mods, NanMode, Parsed, Road};
use crate::refline::RefLine;
use crate::{Error, LoadOptions, Uv, Xy};

/// A loaded OpenCRG road surface.
///
/// Queries take `&self`, so one grid can serve many threads.
#[derive(Clone, Debug)]
pub struct CrgGrid {
    pub(crate) u: UAxis,
    pub(crate) v: VAxis,
    /// Grid values, cross-section major: `z[iu * nv + iv]`.
    pub(crate) z: Vec<f32>,
    /// Constant added to every grid value, from placing the data set at load.
    pub(crate) z_shift: f64,
    pub(crate) ref_z: Profile,
    /// Reference-line height at the end where `ref_z` is constant: `REFERENCE_LINE_END_Z`,
    /// which only end-of-road smoothing reads.
    pub(crate) ref_z_end: f64,
    /// `None` when the file defines no banking.
    pub(crate) bank: Option<Profile>,
    pub(crate) refline: RefLine,
    pub(crate) borders: Borders,
}

/// The uniformly spaced u axis.
#[derive(Clone, Debug)]
pub(crate) struct UAxis {
    pub first: f64,
    pub last: f64,
    pub inc: f64,
    pub n: usize,
}

/// Long-section positions. `first` and `last` bound the core area and clip banking; they
/// come from the header where the C-API takes them from there.
#[derive(Clone, Debug)]
pub(crate) struct VAxis {
    pub nodes: Vec<f64>,
    pub first: f64,
    pub last: f64,
    /// `LONG_SECTION_V_INCREMENT` for numbered long sections, which the C-API locates by
    /// division instead of search.
    pub uniform_inc: Option<f64>,
}

/// A reference-line quantity: one value per cross section, or one constant.
#[derive(Clone, Debug)]
pub(crate) enum Profile {
    Constant(f64),
    Nodes(Vec<f64>),
}

impl Profile {
    /// Linear interpolation between nodes `i` and `i + 1`, in the C-API's operation order.
    #[inline]
    pub fn at(&self, i: usize, frac: f64) -> f64 {
        match self {
            Self::Constant(value) => *value,
            Self::Nodes(values) => values[i] + frac * (values[i + 1] - values[i]),
        }
    }

    /// Change from node `i` to node `i + 1`.
    #[inline]
    pub fn step(&self, i: usize) -> f64 {
        match self {
            Self::Constant(_) => 0.0,
            Self::Nodes(values) => values[i + 1] - values[i],
        }
    }
}

impl CrgGrid {
    /// Loads a `.crg` file from memory.
    ///
    /// Files that include other files (`$ROAD_CRG_FILE`) fail with
    /// [`Error::IncludeUnsupported`]; use [`from_bytes_with`](Self::from_bytes_with) or
    /// [`from_path`](Self::from_path) for those.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        Self::from_bytes_with_options(bytes, &LoadOptions::default())
    }

    /// Loads a `.crg` file from memory with caller-chosen border handling.
    pub fn from_bytes_with_options(bytes: &[u8], options: &LoadOptions) -> Result<Self, Error> {
        let parsed = parse::parse(bytes, &mut |_: &[String]| Err(Error::IncludeUnsupported))?;
        Self::build(parsed, options)
    }

    /// Loads a `.crg` file from memory, calling `loader` for each file it includes, at any
    /// depth. The loader receives the name exactly as the `$ROAD_CRG_FILE` section writes
    /// it, with lines joined and `$VARIABLE` references left in place; resolving it is up
    /// to the loader. Includes nested more than eight levels deep fail.
    pub fn from_bytes_with<F>(
        bytes: &[u8],
        options: &LoadOptions,
        mut loader: F,
    ) -> Result<Self, Error>
    where
        F: FnMut(&str) -> io::Result<Vec<u8>>,
    {
        let mut include = |chain: &[String]| {
            let name = &chain[chain.len() - 1];
            loader(name).map_err(|e| Error::io(name.as_str(), &e))
        };
        Self::build(parse::parse(bytes, &mut include)?, options)
    }

    /// Loads a `.crg` file and the files it includes from disk.
    ///
    /// An include name has each `$VARIABLE` replaced by that environment variable, up to
    /// the next `/`. A relative name is resolved against the directory of the file that
    /// includes it; the C-API resolves it against the working directory instead. A file
    /// that includes itself fails with [`Error::IncludeCycle`].
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::from_path_with_options(path, &LoadOptions::default())
    }

    /// [`from_path`](Self::from_path) with caller-chosen border handling.
    pub fn from_path_with_options(
        path: impl AsRef<Path>,
        options: &LoadOptions,
    ) -> Result<Self, Error> {
        let top = path.as_ref();
        let io_error = |path: &Path, e: io::Error| Error::io(path.display().to_string(), &e);
        let bytes = fs::read(top).map_err(|e| io_error(top, e))?;
        let top_id = fs::canonicalize(top).map_err(|e| io_error(top, e))?;
        let mut include = |chain: &[String]| {
            let mut file = top.to_path_buf();
            let mut seen = vec![top_id.clone()];
            for name in chain {
                let dir = file.parent().unwrap_or(Path::new(""));
                file = dir.join(expand_variables(name)?);
                let id = fs::canonicalize(&file).map_err(|e| io_error(&file, e))?;
                if seen.contains(&id) {
                    return Err(Error::IncludeCycle(file.display().to_string()));
                }
                seen.push(id);
            }
            fs::read(&file).map_err(|e| io_error(&file, e))
        };
        Self::build(parse::parse(&bytes, &mut include)?, options)
    }

    fn build(mut p: Parsed, caller: &LoadOptions) -> Result<Self, Error> {
        let options = p.options.clone().unwrap_or_default();

        if p.nu < 2 {
            return Err(Error::Invalid("fewer than two cross sections"));
        }
        let inc = p.road.u_increment.unwrap_or(0.01);
        if !(inc > 0.0 && inc.is_finite()) {
            return Err(Error::Invalid("reference line increment must be positive"));
        }
        let first = p.road.start_u.unwrap_or(0.0);
        let mut u = UAxis {
            first,
            last: first + inc * (p.nu - 1) as f64,
            inc,
            n: p.nu,
        };
        let mut v = v_axis(&p.road, std::mem::take(&mut p.v), p.v_from_positions)?;

        // Without a $ROAD_CRG_MODS block the C-API applies its default modifiers; a block
        // replaces them entirely (crgOptionMgmt.c:507, crgLoader.c:1021).
        let mods = p.mods.take().unwrap_or_else(|| Mods {
            grid_nan_mode: Some(NanMode::KeepLast),
            refpoint_x: Some(0.0),
            refpoint_y: Some(0.0),
            refpoint_z: Some(0.0),
            refpoint_phi: Some(0.0),
            ..Mods::default()
        });

        // The C-API prepares the data once as loaded, then again after scaling. Only the
        // reference-line height can keep a value from the first pass.
        let loaded_ref_z = ref_line_z(&p.road, &u, p.slope.as_deref());
        let has_bank = p.bank.is_some()
            || p.road.start_bank.is_some_and(|b| b != 0.0)
            || p.road.end_bank.is_some_and(|b| b != 0.0);
        scale(&mut p, &mut u, &mut v, &mods)?;
        let road = &p.road;

        let mut z = p.z;
        if let Some(mode @ (NanMode::SetZero | NanMode::KeepLast)) = mods.grid_nan_mode {
            let offset = mods.grid_nan_offset.unwrap_or(0.0) as f32;
            fill_nans(&mut z, v.nodes.len(), mode, offset);
        }

        let refline = RefLine::new(road, &u, p.phi);
        if refline.closed && options.refline_close_track == Some(true) {
            return Err(Error::Unsupported(
                "closed-track reference line continuation",
            ));
        }
        // calcRefLineZ skips the second pass when scaling zeroed a constant slope.
        let ref_z = if p.slope.is_none() && road.start_slope.unwrap_or(0.0) == 0.0 {
            loaded_ref_z
        } else {
            ref_line_z(road, &u, p.slope.as_deref())
        };
        let bank = has_bank.then(|| match p.bank {
            Some(values) => Profile::Nodes(values),
            None => Profile::Constant(road.start_bank.unwrap_or(0.0)),
        });

        // Placement uses the file's options over the C-API defaults, as the data set does;
        // queries use the file's options over the caller's, as a contact point does.
        let borders = |base: &LoadOptions| Borders {
            u: options.border_mode_u.unwrap_or(base.border_mode_u),
            v: options.border_mode_v.unwrap_or(base.border_mode_v),
            offset_u: options.border_offset_u.unwrap_or(base.border_offset_u),
            offset_v: options.border_offset_v.unwrap_or(base.border_offset_v),
            smooth_begin: smoothing(options.smooth_u_begin, base.smooth_u_begin),
            smooth_end: smoothing(options.smooth_u_end, base.smooth_u_end),
        };
        let mut grid = CrgGrid {
            u,
            v,
            z,
            z_shift: 0.0,
            ref_z,
            ref_z_end: road.end_z.unwrap_or(0.0),
            bank,
            refline,
            borders: borders(&LoadOptions::default()),
        };
        grid.place(road, &mods);
        grid.borders = borders(caller);
        Ok(grid)
    }

    /// Moves the data set as `crgDataApplyTransformations` (crgMgr.c:785) does: either the
    /// reference point lands at the requested position and heading, or the reference line is
    /// offset and rotated.
    fn place(&mut self, road: &Road, mods: &Mods) {
        let refpoint = mods.refpoint_x.is_some()
            || mods.refpoint_y.is_some()
            || mods.refpoint_z.is_some()
            || mods.refpoint_phi.is_some()
            || mods.refpoint_u.is_some()
            || mods.refpoint_u_fraction.is_some()
            || mods.refpoint_v.is_some()
            || mods.refpoint_v_fraction.is_some();

        let (center, angle, shift, dz) = if refpoint {
            let mut u = mods.refpoint_u.unwrap_or(self.u.first);
            if let Some(fraction) = mods.refpoint_u_fraction {
                u = self.u.first + fraction * (self.u.last - self.u.first);
                u += mods.refpoint_u_offset.unwrap_or(0.0);
            }
            let mut v = mods.refpoint_v.unwrap_or(0.0);
            if let Some(fraction) = mods.refpoint_v_fraction {
                v = self.v.first + fraction * (self.v.last - self.v.first);
                v += mods.refpoint_v_offset.unwrap_or(0.0);
            }
            let at = Uv { u, v };
            let from = self.refline.xy(&self.u, at);
            let from_z = self.raw_elevation(at, &self.borders).unwrap_or(0.0);
            let from_phi = self.refline.heading(&self.u, at).phi;
            let shift = Xy {
                x: mods.refpoint_x.unwrap_or(0.0) - from.x,
                y: mods.refpoint_y.unwrap_or(0.0) - from.y,
            };
            let angle = mods.refpoint_phi.unwrap_or(0.0) - from_phi;
            (from, angle, shift, mods.refpoint_z.unwrap_or(0.0) - from_z)
        } else if mods.refline_offset_x.is_some()
            || mods.refline_offset_y.is_some()
            || mods.refline_offset_z.is_some()
            || mods.refline_offset_phi.is_some()
        {
            let from = Xy {
                x: self.refline.first.x,
                y: self.refline.first.y,
            };
            let center = Xy {
                x: mods.refline_rotcenter_x.unwrap_or(from.x),
                y: mods.refline_rotcenter_y.unwrap_or(from.y),
            };
            // The C-API evaluates without options here, so every border refuses.
            let from_z = self
                .raw_elevation(Uv { u: 0.0, v: 0.0 }, &Borders::REFUSE)
                .unwrap_or(0.0);
            // (offset + from) - from, rounded as the C-API rounds it.
            let shift = Xy {
                x: (mods.refline_offset_x.unwrap_or(0.0) + from.x) - from.x,
                y: (mods.refline_offset_y.unwrap_or(0.0) + from.y) - from.y,
            };
            let dz = (mods.refline_offset_z.unwrap_or(0.0) + from_z) - from_z;
            (center, mods.refline_offset_phi.unwrap_or(0.0), shift, dz)
        } else {
            return;
        };

        self.refline.transform(center, angle, shift);
        match &mut self.ref_z {
            Profile::Nodes(values) => values.iter_mut().for_each(|z| *z += dz),
            Profile::Constant(z) if road.start_z.is_some() => {
                *z += dz;
                self.ref_z_end += dz;
            }
            Profile::Constant(_) => self.z_shift += dz,
        }
    }

    /// Grid values, cross-section major: the value at cross section `iu` and long section
    /// `iv` is at `iu * nv + iv`, with `(nu, nv)` from [`dims`](Self::dims). Add
    /// [`z_shift`](Self::z_shift) to get the grid elevation.
    pub fn z_values(&self) -> &[f32] {
        &self.z
    }

    /// Number of cross sections and long sections, `(nu, nv)`.
    pub fn dims(&self) -> (usize, usize) {
        (self.u.n, self.v.nodes.len())
    }

    /// Constant that load added to every grid value to place the data set.
    pub fn z_shift(&self) -> f64 {
        self.z_shift
    }

    /// First and last cross-section position in metres.
    pub fn u_range(&self) -> (f64, f64) {
        (self.u.first, self.u.last)
    }

    /// First and last long-section position in metres.
    pub fn v_range(&self) -> (f64, f64) {
        (self.v.nodes[0], self.v.nodes[self.v.nodes.len() - 1])
    }
}

/// Replaces each `$NAME` in an include name, where the name runs to the next `/`, with the
/// environment variable's value (crgLoader.c:3290).
fn expand_variables(name: &str) -> Result<PathBuf, Error> {
    let mut path = OsString::new();
    let mut rest = name;
    while let Some(at) = rest.find('$') {
        path.push(&rest[..at]);
        let variable = &rest[at + 1..];
        let end = variable.find('/').unwrap_or(variable.len());
        let value = env::var_os(&variable[..end]).ok_or_else(|| Error::Io {
            path: name.to_owned(),
            kind: io::ErrorKind::NotFound,
            message: format!("environment variable {} is not set", &variable[..end]),
        })?;
        path.push(value);
        rest = &variable[end..];
    }
    path.push(rest);
    Ok(PathBuf::from(path))
}

/// A smoothing zone from the file, else from the caller. The C-API ignores zones that are
/// not positive.
fn smoothing(file: Option<f64>, caller: Option<f64>) -> Option<f64> {
    file.or(caller).filter(|&zone| zone > 0.0)
}

/// Applies the scaling modifiers as `crgDataSetModifiersApply` (crgMgr.c:503) does, and
/// drops the header end values that scaling invalidates.
fn scale(p: &mut Parsed, u: &mut UAxis, v: &mut VAxis, mods: &Mods) -> Result<(), Error> {
    let factors = [
        mods.scale_z_grid,
        mods.scale_slope,
        mods.scale_banking,
        mods.scale_length,
        mods.scale_width,
        mods.scale_curvature,
    ];
    if factors.iter().flatten().any(|k| !k.is_finite()) {
        return Err(Error::Invalid("scale factors must be finite"));
    }
    if [mods.scale_length, mods.scale_width]
        .iter()
        .flatten()
        .any(|&k| k <= 0.0)
    {
        return Err(Error::Invalid(
            "length and width scale factors must be positive",
        ));
    }
    let road = &mut p.road;
    let times = |value: &mut Option<f64>, k: f64| {
        if let Some(value) = value {
            *value *= k;
        }
    };

    if let Some(k) = mods.scale_z_grid {
        let k = k as f32;
        p.z.iter_mut().for_each(|z| *z *= k);
    }
    if let Some(k) = mods.scale_slope {
        if let Some(slope) = &mut p.slope {
            slope.iter_mut().for_each(|s| *s *= k);
        }
        times(&mut road.start_slope, k);
        times(&mut road.end_slope, k);
        road.end_z = None;
    }
    if let Some(k) = mods.scale_banking {
        if let Some(bank) = &mut p.bank {
            bank.iter_mut().for_each(|b| *b *= k);
        }
        times(&mut road.start_bank, k);
        times(&mut road.end_bank, k);
    }
    if let Some(k) = mods.scale_length {
        u.last = u.first + k * (u.last - u.first);
        u.inc *= k;
        road.end_x = None;
        road.end_y = None;
        road.end_z = None;
    }
    if let Some(k) = mods.scale_width {
        v.nodes.iter_mut().for_each(|node| *node *= k);
        v.first *= k;
        v.last *= k;
        if let Some(inc) = &mut v.uniform_inc {
            *inc *= k;
        }
    }
    if let Some(k) = mods.scale_curvature {
        // Headings turn relative to the start heading; the first node keeps its value.
        let first = road.start_phi.unwrap_or(0.0);
        if let Some(phi) = &mut p.phi {
            phi.iter_mut()
                .skip(1)
                .for_each(|phi| *phi = first + k * (*phi - first));
        }
        road.end_phi = None;
        road.end_x = None;
        road.end_y = None;
    }
    Ok(())
}

/// The v axis as `prepareFromPosDef` and `prepareFromIndexDef` leave it. Where the C-API
/// would fall back to 0 for a missing `LONG_SECTION_V_RIGHT` or `_LEFT`, the nodes are used.
fn v_axis(road: &Road, nodes: Vec<f64>, from_positions: bool) -> Result<VAxis, Error> {
    let (node_first, node_last) = (nodes[0], nodes[nodes.len() - 1]);
    let last = road.v_left.unwrap_or(node_last);
    if from_positions {
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        for w in nodes.windows(2) {
            min = min.min(w[1] - w[0]);
            max = max.max(w[1] - w[0]);
        }
        let first = if (max - min) / min < 1e-3 {
            node_first
        } else {
            road.v_right.unwrap_or(node_first)
        };
        Ok(VAxis {
            nodes,
            first,
            last,
            uniform_inc: None,
        })
    } else {
        let inc = road.v_increment.unwrap_or(0.01);
        if !(inc > 0.0 && inc.is_finite()) {
            return Err(Error::Invalid("long section increment must be positive"));
        }
        Ok(VAxis {
            nodes,
            first: node_first,
            last,
            uniform_inc: Some(inc),
        })
    }
}

/// Replaces NaN grid values within each cross section, as `crgLoaderHandleNaNs` does:
/// first sweeping toward the left, then back toward the right.
fn fill_nans(z: &mut [f32], nv: usize, mode: NanMode, offset: f32) {
    // The C-API resets this flag only before the second sweep, so it carries from one
    // cross section's second sweep into the next one's first sweep.
    let mut offset_applied = false;
    for section in z.chunks_exact_mut(nv) {
        for iv in 1..nv {
            if !section[iv].is_nan() {
                continue;
            }
            match mode {
                NanMode::SetZero => section[iv] = offset,
                NanMode::KeepLast => {
                    section[iv] = section[iv - 1];
                    if !section[iv].is_nan() && !offset_applied {
                        section[iv] += offset;
                        offset_applied = true;
                    }
                }
                NanMode::Keep => {}
            }
        }
        offset_applied = false;
        for iv in (1..nv).rev() {
            if !(section[iv - 1].is_nan() && !section[iv].is_nan()) {
                continue;
            }
            match mode {
                NanMode::SetZero => section[iv - 1] = offset,
                NanMode::KeepLast => {
                    section[iv - 1] = section[iv];
                    if !offset_applied {
                        section[iv - 1] += offset;
                        offset_applied = true;
                    }
                }
                NanMode::Keep => {}
            }
        }
    }
}

/// Reference-line elevation integrated from slope, as `calcRefLineZ` does. Integrates
/// backward from `REFERENCE_LINE_END_Z` and blends forward when that header is present.
fn ref_line_z(road: &Road, u: &UAxis, slopes: Option<&[f64]>) -> Profile {
    let start_z = road.start_z.unwrap_or(0.0);
    let start_slope = road.start_slope.unwrap_or(0.0);
    if slopes.is_none() && start_slope == 0.0 {
        return Profile::Constant(start_z);
    }
    let slope = |i: usize| slopes.map_or(start_slope, |s| s[i]);
    let n = u.n;
    let mut z = vec![0.0; n];

    if let Some(end_z) = road.end_z {
        z[n - 1] = end_z;
        for i in (1..n).rev() {
            z[i - 1] = z[i] - slope(i) * u.inc;
        }
        z[0] = start_z;
        // The weight uses i where i + 1 would reach end_z; kept for parity.
        for i in 0..n - 2 {
            let fraction = i as f64 / (n - 1) as f64;
            z[i + 1] = (1.0 - fraction) * (z[i] + slope(i + 1) * u.inc) + fraction * z[i + 1];
        }
    } else {
        z[0] = start_z;
        for i in 0..n - 1 {
            z[i + 1] = z[i] + slope(i + 1) * u.inc;
        }
    }
    Profile::Nodes(z)
}
