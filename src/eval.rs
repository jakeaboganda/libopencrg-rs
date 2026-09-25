//! Elevation queries, following `crgDataEvaluv2z` (crgEvalz.c:64).

use crate::grid::{CrgGrid, Profile};
use crate::{BorderMode, GridSample, Heading, Normal, Uv, Xy};

/// Border handling for both axes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Borders {
    pub u: BorderMode,
    pub v: BorderMode,
    /// Added to elevations beyond the u border, or replacing them in `Zero` mode.
    pub offset_u: f64,
    pub offset_v: f64,
    /// Smoothing zone lengths, positive where set.
    pub smooth_begin: Option<f64>,
    pub smooth_end: Option<f64>,
}

impl Borders {
    /// The C-API's behaviour when called without an option list.
    pub const REFUSE: Self = Self {
        u: BorderMode::None,
        v: BorderMode::None,
        offset_u: 0.0,
        offset_v: 0.0,
        smooth_begin: None,
        smooth_end: None,
    };
}

/// Where a query lands and which parts of the elevation it takes.
struct Cell {
    iu: usize,
    fu: f64,
    /// d(fu)/du, or 0 where a border holds the value.
    su: f64,
    iv: usize,
    fv: f64,
    /// d(fv)/dv, or 0 where a border holds the value.
    sv: f64,
    /// `false` beyond a `Zero` border, where the grid contributes nothing.
    grid: bool,
    /// `false` beyond a `Zero` u border.
    bank: bool,
    /// v for banking, before clipping to the core area, and its derivative by v.
    bank_v: f64,
    bank_sv: f64,
    border: Border,
    offset: f64,
    /// u after `Repeat` or `Reflect`, and its derivative by u.
    u: f64,
    u_sign: f64,
    /// Whether u lies in the core area after `Repeat` or `Reflect`.
    in_u: bool,
    /// The end whose reference-line height a `Keep` or `Zero` u border smooths toward.
    smooth_side: Option<Side>,
}

/// How the border offset combines with the elevation.
#[derive(PartialEq)]
enum Border {
    Inside,
    Add,
    Replace,
}

#[derive(Clone, Copy)]
enum Side {
    Begin,
    End,
}

/// Tolerance for positions just outside a positioned long section (crgEvalz.c:30).
const MAX_BORDER_ERROR: f64 = 1.0e-8;

impl CrgGrid {
    /// Grid elevation and slopes at `uv`, without reference-line height, slope, bank,
    /// smoothing, or load-time shift. Returns `None` where a `None` border refuses the query
    /// or the grid has a NaN hole.
    #[inline]
    pub fn grid_at_uv(&self, uv: Uv) -> Option<GridSample> {
        let cell = self.locate(uv, &self.borders)?;
        if !cell.grid {
            return Some(GridSample::default());
        }
        let (c, d10, d01, z00) = self.corners(&cell);
        let z = (c * cell.fv + d10) * cell.fu + d01 * cell.fv + z00;
        if z.is_nan() {
            return None;
        }
        Some(GridSample {
            z,
            dz_du: (c * cell.fv + d10) * cell.su,
            dz_dv: (c * cell.fu + d01) * cell.sv,
        })
    }

    /// Road elevation at `uv` in metres: reference-line height, bank times v, and grid
    /// elevation, with border modes, offsets, and smoothing applied. Returns `None` where a
    /// `None` border refuses the query or the grid has a NaN hole.
    #[inline]
    pub fn elevation_at_uv(&self, uv: Uv) -> Option<f64> {
        self.raw_elevation(uv, &self.borders)
            .filter(|z| !z.is_nan())
    }

    /// Global position of a grid position. Beyond either end of the reference line, the line
    /// continues straight along its end heading.
    #[inline]
    pub fn xy_from_uv(&self, uv: Uv) -> Xy {
        self.refline.xy(&self.u, uv)
    }

    /// Grid position of a global position, as the C-API finds it with an empty search
    /// history: start from the nearest of every tenth reference-line node, then walk to the
    /// segment whose mitred normals enclose `xy`. Beyond either end, u and v are measured
    /// along the end heading. Returns `None` for non-finite input or a degenerate segment.
    ///
    /// Where the grid overlaps itself, several positions map to `xy` and the search returns
    /// one of them; which one depends on the start node.
    pub fn uv_from_xy(&self, xy: Xy) -> Option<Uv> {
        let start = self.refline.coarse_index(xy);
        self.finite_uv(xy, start)
    }

    /// Like [`uv_from_xy`](Self::uv_from_xy), but starts the search at `hint`, typically the
    /// previous result for a moving point. This is much faster on long roads and stays on the
    /// same branch where the grid overlaps itself. A hint whose position is 2.2 m or more
    /// from `xy` is ignored, as the C-API ignores history that far away.
    pub fn uv_from_xy_near(&self, xy: Xy, hint: Uv) -> Option<Uv> {
        let from = self.xy_from_uv(hint);
        let (dx, dy) = (xy.x - from.x, xy.y - from.y);
        // The C-API's default dCrgCpOptionRefLineFar, squared.
        let far = 2.2 * 2.2;
        let start = if dx * dx + dy * dy < far {
            // The C-API remembers the node after the segment it found.
            let segment = ((hint.u - self.u.first) / self.u.inc).floor();
            (segment.clamp(0.0, (self.u.n - 2) as f64) as usize + 2).min(self.u.n - 1)
        } else {
            self.refline.coarse_index(xy)
        };
        self.finite_uv(xy, start)
    }

    fn finite_uv(&self, xy: Xy, start: usize) -> Option<Uv> {
        if !(xy.x.is_finite() && xy.y.is_finite()) {
            return None;
        }
        let uv = self.refline.uv(&self.u, xy, start);
        (uv.u.is_finite() && uv.v.is_finite()).then_some(uv)
    }

    /// Heading of the reference line at `uv.u`, and curvature of the line through `uv`
    /// parallel to it. The C-API measures curvature over sections of at least 0.5 m and
    /// reports 0 within that distance of either end.
    #[inline]
    pub fn heading_at_uv(&self, uv: Uv) -> Heading {
        self.refline.heading(&self.u, uv)
    }

    /// Unit upward normal at `uv` of the surface traced by [`xy_from_uv`](Self::xy_from_uv)
    /// and [`elevation_at_uv`](Self::elevation_at_uv), exact within the evaluated grid cell
    /// and reference-line segment. Returns `None` where the elevation is `None`, and where
    /// the xy mapping folds over itself, at or beyond the reference line's centre of
    /// curvature.
    pub fn normal_at_uv(&self, uv: Uv) -> Option<Normal> {
        let (z, dz_du, dz_dv) = self.surface::<true>(uv, &self.borders)?;
        if z.is_nan() {
            return None;
        }

        // Normal of the surface (x, y, z)(u, v): the cross product of its u and v tangents.
        let (xy_du, xy_dv) = self.refline.jacobian(&self.u, uv);
        let upward = xy_du[0] * xy_dv[1] - xy_du[1] * xy_dv[0];
        if upward <= 0.0 || upward.is_nan() {
            return None;
        }
        let n = [
            xy_du[1] * dz_dv - dz_du * xy_dv[1],
            dz_du * xy_dv[0] - xy_du[0] * dz_dv,
            upward,
        ];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        Some(Normal {
            x: n[0] / length,
            y: n[1] / length,
            z: n[2] / length,
        })
    }

    /// Elevation exactly as the C-API computes it, NaN included.
    #[inline]
    pub(crate) fn raw_elevation(&self, uv: Uv, borders: &Borders) -> Option<f64> {
        self.surface::<false>(uv, borders).map(|(z, _, _)| z)
    }

    /// Elevation and, when `SLOPES` is set, its derivatives by u and v. The elevation
    /// follows the operation order of `crgDataEvaluv2z`.
    #[inline]
    fn surface<const SLOPES: bool>(&self, uv: Uv, borders: &Borders) -> Option<(f64, f64, f64)> {
        let cell = self.locate(uv, borders)?;
        let (mut z, mut dz_du, mut dz_dv) = (0.0, 0.0, 0.0);
        if cell.grid {
            let (c, d10, d01, z00) = self.corners(&cell);
            z = (c * cell.fv + d10) * cell.fu + d01 * cell.fv + z00;
            z += self.z_shift;
            if SLOPES {
                dz_du = (c * cell.fv + d10) * cell.su;
                dz_dv = (c * cell.fu + d01) * cell.sv;
            }
        }
        let smooth = self.smoothing(&cell, borders);

        z += self.ref_z.at(cell.iu, cell.fu);
        if SLOPES {
            dz_du += self.ref_z.step(cell.iu) * cell.su;
        }
        if let (Some(bank), true) = (&self.bank, cell.bank) {
            let v = cell.bank_v.clamp(self.v.first, self.v.last);
            let b = bank.at(cell.iu, cell.fu);
            z += b * v;
            if SLOPES {
                dz_du += bank.step(cell.iu) * cell.su * v;
                if v == cell.bank_v {
                    dz_dv += b * cell.bank_sv;
                }
            }
        }
        match cell.border {
            Border::Inside => {}
            Border::Add => z += cell.offset,
            Border::Replace => {
                z = cell.offset;
                (dz_du, dz_dv) = (0.0, 0.0);
            }
        }
        if let Some((base, scale, scale_du)) = smooth {
            if SLOPES {
                dz_du = dz_du * scale + (z - base) * scale_du;
                dz_dv *= scale;
            }
            z = base + (z - base) * scale;
        }
        Some((z, dz_du, dz_dv))
    }

    /// Base height, scale, and d(scale)/du of the smoothing zone `cell` lies in, as
    /// crgEvalz.c:502-558 computes them.
    #[inline]
    fn smoothing(&self, cell: &Cell, borders: &Borders) -> Option<(f64, f64, f64)> {
        if borders.smooth_begin.is_none() && borders.smooth_end.is_none() {
            return None;
        }
        let (first, last) = (self.u.first, self.u.last);
        let mut zone = None;
        if cell.in_u || cell.smooth_side.is_some() {
            if let Some(length) = borders.smooth_begin.filter(|&l| cell.u - first <= l) {
                zone = Some(if cell.u < first {
                    (Side::Begin, 0.0, 0.0)
                } else {
                    (Side::Begin, (cell.u - first) / length, cell.u_sign / length)
                });
            }
            if let Some(length) = borders.smooth_end.filter(|&l| last - cell.u <= l) {
                zone = Some(if cell.u > last {
                    (Side::End, 0.0, 0.0)
                } else {
                    (Side::End, (last - cell.u) / length, -cell.u_sign / length)
                });
            }
        }
        let (side, scale, scale_du) = zone?;
        let base = match (side, &self.ref_z) {
            (Side::Begin, Profile::Nodes(values)) => values[0],
            (Side::End, Profile::Nodes(values)) => values[values.len() - 1],
            (Side::Begin, Profile::Constant(z)) => *z,
            (Side::End, Profile::Constant(_)) => self.ref_z_end,
        };
        Some((base, scale, scale_du))
    }

    /// Bilinear coefficients in the operation order of crgEvalz.c:489-495.
    #[inline]
    fn corners(&self, cell: &Cell) -> (f64, f64, f64, f64) {
        let nv = self.v.nodes.len();
        let a = cell.iu * nv + cell.iv;
        let z = &self.z[a..=a + nv + 1];
        let z00 = f64::from(z[0]);
        let d10 = f64::from(z[nv]) - z00;
        let z01 = f64::from(z[1]);
        let c = f64::from(z[nv + 1]) - (d10 + z01);
        (c, d10, z01 - z00, z00)
    }

    #[inline(always)]
    fn locate(&self, uv: Uv, borders: &Borders) -> Option<Cell> {
        let mut grid = true;
        let mut bank = true;
        let mut offset = 0.0;

        let u = &self.u;
        let mut at_u = uv.u;
        let mut u_sign = 1.0;
        let mut smooth_side = None;
        let mut fu = (uv.u - u.first) / u.inc;
        let mut in_u = !(uv.u < u.first || uv.u > u.last);
        if !in_u {
            let side = if fu < 0.0 { Side::Begin } else { Side::End };
            match borders.u {
                BorderMode::None => return None,
                BorderMode::Zero => {
                    grid = false;
                    bank = false;
                    smooth_side = Some(side);
                }
                BorderMode::Keep => smooth_side = Some(side),
                BorderMode::Repeat | BorderMode::Reflect => {
                    u_sign = wrap(&mut fu, uv.u - u.first, u.last - u.first, u.inc, borders.u);
                    in_u = true;
                    at_u = u.first + fu * u.inc;
                }
            }
            offset += borders.offset_u;
        }
        let (iu, clamped) = split(&mut fu, u.n);
        let su = if clamped { 0.0 } else { u_sign / u.inc };

        let v = &self.v;
        let mut mode_v = borders.v;
        let mut in_v = true;
        let mut bank_v = uv.v;
        let mut bank_sv = 1.0;
        let (iv, fv, sv);
        if let Some(inc) = v.uniform_inc {
            let mut f = (uv.v - v.first) / inc;
            let mut sign = 1.0;
            if uv.v < v.first || uv.v > v.last {
                in_v = false;
                match mode_v {
                    BorderMode::None => return None,
                    BorderMode::Zero => grid = false,
                    BorderMode::Keep => {}
                    BorderMode::Repeat | BorderMode::Reflect => {
                        sign = wrap(&mut f, uv.v - v.first, v.last - v.first, inc, mode_v);
                        in_v = true;
                        bank_v = v.first + f * inc;
                        bank_sv = sign;
                    }
                }
                offset += borders.offset_v;
            }
            let (i, clamped) = split(&mut f, v.nodes.len());
            (iv, fv, sv) = (i, f, if clamped { 0.0 } else { sign / inc });
        } else {
            let mut pos = uv.v;
            let mut sign = 1.0;
            if pos < v.first || pos > v.last {
                in_v = false;
                if (pos - v.first).abs() < MAX_BORDER_ERROR {
                    pos = v.first;
                    mode_v = BorderMode::Keep;
                } else if (pos - v.last).abs() < MAX_BORDER_ERROR {
                    pos = v.last;
                    mode_v = BorderMode::Keep;
                } else {
                    match mode_v {
                        BorderMode::None => return None,
                        BorderMode::Zero => grid = false,
                        BorderMode::Keep => {}
                        BorderMode::Repeat | BorderMode::Reflect => {
                            sign = wrap_positioned(&mut pos, v.first, v.last, mode_v);
                            in_v = true;
                        }
                    }
                }
                offset += borders.offset_v;
            }
            let nodes = &v.nodes;
            let i = nodes[1..nodes.len() - 1].partition_point(|&node| node <= pos);
            let width = nodes[i + 1] - nodes[i];
            let f = (pos - nodes[i]) / width;
            let clamped = f.clamp(0.0, 1.0);
            (iv, fv, sv) = (i, clamped, if clamped == f { sign / width } else { 0.0 });
        }

        let border = if !in_u {
            border_kind(borders.u)
        } else if !in_v {
            border_kind(mode_v)
        } else {
            Border::Inside
        };
        Some(Cell {
            iu,
            fu,
            su,
            iv,
            fv,
            sv,
            grid,
            bank,
            bank_v,
            bank_sv,
            border,
            offset,
            u: at_u,
            u_sign,
            in_u,
            smooth_side,
        })
    }
}

fn border_kind(mode: BorderMode) -> Border {
    match mode {
        BorderMode::Zero => Border::Replace,
        BorderMode::Keep => Border::Add,
        _ => Border::Inside,
    }
}

/// Maps a node coordinate `frac` beyond a uniformly spaced axis back onto it, for `Repeat`
/// or `Reflect` (crgEvalz.c:159-190, 257-290). `from_first` is the distance from the first
/// node and `size` the axis length. Returns the sign of the mapping's slope, 1 or -1.
fn wrap(frac: &mut f64, from_first: f64, size: f64, inc: f64, mode: BorderMode) -> f64 {
    let max = size / inc;
    if mode == BorderMode::Repeat {
        *frac %= max;
        if *frac < 0.0 {
            *frac += max;
        }
        return 1.0;
    }
    // The C-API truncates to int; parity only matters within that range.
    let sequence = (from_first / size).trunc().abs();
    let sign = if *frac < 0.0 { -1.0 } else { 1.0 };
    *frac = frac.abs() - sequence * max;
    if sequence % 2.0 == 1.0 {
        *frac = max - *frac;
        -sign
    } else {
        sign
    }
}

/// `Repeat` or `Reflect` for positioned long sections (crgEvalz.c:363-409). Returns the
/// derivative of the new position by the old one.
fn wrap_positioned(pos: &mut f64, first: f64, last: f64, mode: BorderMode) -> f64 {
    let range = last - first;
    if mode == BorderMode::Repeat {
        if *pos > last {
            *pos = first + (*pos - last) % range;
        } else if *pos < first {
            *pos = last + (*pos - first) % range;
        }
        // Again allow for rounding at the edges.
        if (*pos - first).abs() < MAX_BORDER_ERROR {
            *pos = first;
        } else if (*pos - last).abs() < MAX_BORDER_ERROR {
            *pos = last;
        }
        return 1.0;
    }
    if *pos > last {
        let remainder = (*pos - last) % range;
        let sequence = ((*pos - last) / range).trunc().abs();
        if sequence % 2.0 == 0.0 {
            *pos = last - remainder;
            -1.0
        } else {
            *pos = first + remainder;
            1.0
        }
    } else {
        let remainder = (*pos - first) % range;
        let sequence = ((*pos - first) / range).trunc().abs();
        if sequence % 2.0 == 1.0 {
            *pos = last + remainder;
            1.0
        } else {
            *pos = first - remainder;
            -1.0
        }
    }
}

/// Splits a node coordinate into a cell index and the fraction within that cell, clamped
/// to the axis. Returns `true` when the coordinate lay beyond either end.
#[inline]
fn split(frac: &mut f64, n: usize) -> (usize, bool) {
    let beyond = *frac < 0.0 || *frac > (n - 1) as f64;
    if *frac < 0.0 {
        *frac = 0.0;
    }
    // frac >= 0 or NaN here, so `as` truncates toward zero and maps NaN to 0.
    let i = *frac as usize;
    if i >= n - 1 {
        *frac = 1.0;
        (n - 2, beyond)
    } else {
        *frac -= i as f64;
        (i, beyond)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, LoadOptions};
    use std::path::Path;

    fn fixture(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/crg-txt")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// Fixture `name` with `block` inserted before its `$ROAD_CRG` section.
    fn fixture_with(name: &str, block: &str) -> Vec<u8> {
        let text = String::from_utf8(fixture(name)).unwrap();
        let at = text.find("$ROAD_CRG ").unwrap();
        format!("{}{block}\n$\n{}", &text[..at], &text[at..]).into_bytes()
    }

    fn straight_with(block: &str) -> Vec<u8> {
        fixture_with("handmade_straight.crg", block)
    }

    fn uv(u: f64, v: f64) -> Uv {
        Uv { u, v }
    }

    #[test]
    fn grid_slopes_match_differences_within_a_cell() {
        let grid = CrgGrid::from_bytes(&fixture("handmade_curved_banked_sloped.crg")).unwrap();
        let (u, v, h) = (7.3, 0.2, 1e-4);
        let at = |u, v| grid.grid_at_uv(uv(u, v)).unwrap();
        let sample = at(u, v);
        let dz_du = (at(u + h, v).z - at(u - h, v).z) / (2.0 * h);
        let dz_dv = (at(u, v + h).z - at(u, v - h).z) / (2.0 * h);
        assert!((sample.dz_du - dz_du).abs() < 1e-9, "{sample:?} vs {dz_du}");
        assert!((sample.dz_dv - dz_dv).abs() < 1e-9, "{sample:?} vs {dz_dv}");
        assert!(sample.dz_du != 0.0 && sample.dz_dv != 0.0);
    }

    #[test]
    fn held_borders_have_zero_slope() {
        let grid = CrgGrid::from_bytes(&fixture("handmade_curved_banked_sloped.crg")).unwrap();
        let before = grid.grid_at_uv(uv(-3.0, 0.2)).unwrap();
        let edge = grid.grid_at_uv(uv(0.0, 0.2)).unwrap();
        assert_eq!((before.z, before.dz_du), (edge.z, 0.0));
        let left = grid.grid_at_uv(uv(7.3, 9.0)).unwrap();
        assert_eq!(left.dz_dv, 0.0);
    }

    #[test]
    fn zero_and_none_borders() {
        let bytes = fixture("handmade_straight.crg");
        let options = LoadOptions {
            border_mode_u: BorderMode::Zero,
            border_mode_v: BorderMode::None,
            ..LoadOptions::default()
        };
        let grid = CrgGrid::from_bytes_with_options(&bytes, &options).unwrap();
        assert_eq!(grid.grid_at_uv(uv(-1.0, 0.0)), Some(GridSample::default()));
        assert_eq!(grid.grid_at_uv(uv(5.0, 2.0)), None);
        assert_eq!(grid.elevation_at_uv(uv(5.0, 2.0)), None);
    }

    #[test]
    fn nan_holes_return_none() {
        // Cross sections 7 and 8 of handmade_straight.crg start with a NaN.
        let grid =
            CrgGrid::from_bytes(&straight_with("$ROAD_CRG_MODS\ngrid_nan_mode = 0")).unwrap();
        let (_, nv) = grid.dims();
        assert!(grid.z_values()[7 * nv].is_nan());
        assert_eq!(grid.elevation_at_uv(uv(7.5, -1.4)), None);
        assert_eq!(grid.grid_at_uv(uv(7.5, -1.4)), None);
        assert!(grid.elevation_at_uv(uv(5.5, -1.4)).is_some());

        // The default modifiers fill it from its neighbour.
        let grid = CrgGrid::from_bytes(&fixture("handmade_straight.crg")).unwrap();
        assert!(grid.z_values().iter().all(|z| !z.is_nan()));
    }

    #[test]
    fn rejected_settings_are_named() {
        let circle = fixture_with(
            "handmade_circle.crg",
            "$ROAD_CRG_OPTS\nrefline_continuation = 1",
        );
        assert_eq!(
            CrgGrid::from_bytes(&circle).unwrap_err(),
            Error::Unsupported("closed-track reference line continuation")
        );
        // The straight line is not closed, so continuation changes nothing.
        assert!(
            CrgGrid::from_bytes(&straight_with("$ROAD_CRG_OPTS\nrefline_continuation = 1")).is_ok()
        );
        for mods in ["scale_length = 0", "scale_width = -1"] {
            let bytes = straight_with(&format!("$ROAD_CRG_MODS\n{mods}"));
            assert!(
                matches!(CrgGrid::from_bytes(&bytes), Err(Error::Invalid(_))),
                "{mods}"
            );
        }
    }

    /// Largest distance between `normal_at_uv` and the normal of the surface traced by
    /// `xy_from_uv` and `elevation_at_uv`, from central differences.
    fn normal_error(name: &str) -> f64 {
        let grid = CrgGrid::from_bytes(&fixture(name)).unwrap();
        let points = [
            (7.3, 0.2),
            (12.6, -1.2),
            (15.25, 1.3),
            (3.7, 0.7),
            (18.1, -0.4),
        ];
        surface_normal_error(&grid, &points)
    }

    fn surface_normal_error(grid: &CrgGrid, points: &[(f64, f64)]) -> f64 {
        let point = |u, v| {
            let xy = grid.xy_from_uv(uv(u, v));
            [xy.x, xy.y, grid.elevation_at_uv(uv(u, v)).unwrap()]
        };
        let h = 1e-6;
        let mut worst: f64 = 0.0;
        for &(u, v) in points {
            let (a, b) = (point(u + h, v), point(u - h, v));
            let (c, d) = (point(u, v + h), point(u, v - h));
            let tu: [f64; 3] = std::array::from_fn(|i| (a[i] - b[i]) / (2.0 * h));
            let tv: [f64; 3] = std::array::from_fn(|i| (c[i] - d[i]) / (2.0 * h));
            let n = [
                tu[1] * tv[2] - tu[2] * tv[1],
                tu[2] * tv[0] - tu[0] * tv[2],
                tu[0] * tv[1] - tu[1] * tv[0],
            ];
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let got = grid.normal_at_uv(uv(u, v)).unwrap();
            let error = ((got.x - n[0] / length).powi(2)
                + (got.y - n[1] / length).powi(2)
                + (got.z - n[2] / length).powi(2))
            .sqrt();
            worst = worst.max(error);
        }
        worst
    }

    #[test]
    fn normals_match_the_surface() {
        // Exact up to rounding in the differences.
        assert!(normal_error("handmade_sloped.crg") < 1e-9);
        assert!(normal_error("handmade_banked.crg") < 1e-9);
        assert!(normal_error("handmade_curved_banked_sloped.crg") < 1e-9);
        assert!(normal_error("handmade_circle.crg") < 1e-9);
    }

    #[test]
    fn normals_follow_wrapped_borders_and_smoothing() {
        let opts = "$ROAD_CRG_OPTS\nborder_mode_u = 4\nborder_mode_v = 4\n\
                    border_smooth_ubeg = 3\nborder_smooth_uend = 2";
        // Points in reflected copies an odd number of lengths away, and in smoothing zones.
        let points = [
            (-30.3, 4.3),
            (-8.2, -5.1),
            (41.6, 0.7),
            (1.3, 0.35),
            (20.7, -0.85),
        ];
        for name in ["handmade_straight.crg", "handmade_curved_banked_sloped.crg"] {
            let grid = CrgGrid::from_bytes(&fixture_with(name, opts)).unwrap();
            assert!(surface_normal_error(&grid, &points) < 1e-8, "{name}");
        }
        let repeat = opts.replace('4', "3");
        let grid = CrgGrid::from_bytes(&straight_with(&repeat)).unwrap();
        assert!(surface_normal_error(&grid, &points) < 1e-8);
    }

    #[test]
    fn normal_on_a_flat_straight_road_points_up() {
        let grid = CrgGrid::from_bytes(&fixture("handmade_straight.crg")).unwrap();
        // Cross section 0 is flat at v = 0.
        let n = grid.normal_at_uv(uv(0.5, 0.0)).unwrap();
        assert!((n.x.powi(2) + n.y.powi(2) + n.z.powi(2) - 1.0).abs() < 1e-15);
        assert!(n.z > 0.99);
        // Beyond the centre of curvature there is no normal.
        let circle = CrgGrid::from_bytes(&fixture("handmade_circle.crg")).unwrap();
        let heading = circle.heading_at_uv(uv(10.0, 0.0));
        assert!(heading.curvature > 0.0);
        let beyond = 1.0 / heading.curvature + 1.0;
        assert_eq!(circle.normal_at_uv(uv(10.0, beyond)), None);
    }

    #[test]
    fn xy_to_uv_round_trips() {
        let grid = CrgGrid::from_bytes(&fixture("handmade_curved_banked_sloped.crg")).unwrap();
        let mut hint = uv(0.0, 0.0);
        for i in 0..=40 {
            let want = uv(0.5 * i as f64, 1.2 * (0.3 * i as f64).sin());
            let xy = grid.xy_from_uv(want);
            for got in [grid.uv_from_xy(xy), grid.uv_from_xy_near(xy, hint)] {
                let got = got.unwrap();
                assert!((got.u - want.u).abs() < 1e-9 && (got.v - want.v).abs() < 1e-9);
            }
            hint = want;
        }
        let nan = Xy {
            x: f64::NAN,
            y: 0.0,
        };
        assert_eq!(grid.uv_from_xy(nan), None);
        assert_eq!(grid.uv_from_xy_near(nan, hint), None);
    }

    #[test]
    fn nan_queries_return_none() {
        let grid = CrgGrid::from_bytes(&fixture("handmade_straight.crg")).unwrap();
        assert_eq!(grid.elevation_at_uv(uv(f64::NAN, 0.0)), None);
        assert_eq!(grid.elevation_at_uv(uv(1.0, f64::NAN)), None);
    }
}
