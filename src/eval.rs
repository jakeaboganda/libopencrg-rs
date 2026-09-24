//! Elevation queries, following `crgDataEvaluv2z` (crgEvalz.c:64).

use crate::grid::CrgGrid;
use crate::{BorderMode, GridSample, Uv};

/// Border handling for both axes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Borders {
    pub u: BorderMode,
    pub v: BorderMode,
    /// Added to elevations beyond the u border, or replacing them in `Zero` mode.
    pub offset_u: f64,
    pub offset_v: f64,
}

impl Borders {
    /// The C-API's behaviour when called without an option list.
    pub const REFUSE: Self = Self {
        u: BorderMode::None,
        v: BorderMode::None,
        offset_u: 0.0,
        offset_v: 0.0,
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
    border: Border,
    offset: f64,
}

/// How the border offset combines with the elevation.
#[derive(PartialEq)]
enum Border {
    Inside,
    Add,
    Replace,
}

/// Tolerance for positions just outside a positioned long section (crgEvalz.c:30).
const MAX_BORDER_ERROR: f64 = 1.0e-8;

impl CrgGrid {
    /// Grid elevation and slopes at `uv`, without reference-line height, slope, bank, or
    /// load-time shift. Returns `None` where a `None` border refuses the query or the grid
    /// has a NaN hole.
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
    /// elevation, with border modes and offsets applied. Returns `None` where a `None` border
    /// refuses the query or the grid has a NaN hole.
    #[inline]
    pub fn elevation_at_uv(&self, uv: Uv) -> Option<f64> {
        self.raw_elevation(uv, &self.borders)
            .filter(|z| !z.is_nan())
    }

    /// Elevation exactly as the C-API computes it, NaN included.
    pub(crate) fn raw_elevation(&self, uv: Uv, borders: &Borders) -> Option<f64> {
        let cell = self.locate(uv, borders)?;
        let mut z = 0.0;
        if cell.grid {
            let (c, d10, d01, z00) = self.corners(&cell);
            z = (c * cell.fv + d10) * cell.fu + d01 * cell.fv + z00;
            z += self.z_shift;
        }
        z += self.ref_z.at(cell.iu, cell.fu);
        if let Some(bank) = &self.bank
            && cell.bank
        {
            let v = uv.v.clamp(self.v.first, self.v.last);
            z += bank.at(cell.iu, cell.fu) * v;
        }
        match cell.border {
            Border::Inside => {}
            Border::Add => z += cell.offset,
            Border::Replace => z = cell.offset,
        }
        Some(z)
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

    fn locate(&self, uv: Uv, borders: &Borders) -> Option<Cell> {
        let mut grid = true;
        let mut bank = true;
        let mut offset = 0.0;

        let u = &self.u;
        let mut fu = (uv.u - u.first) / u.inc;
        let mut su = 1.0 / u.inc;
        let in_u = !(uv.u < u.first || uv.u > u.last);
        if !in_u {
            match borders.u {
                BorderMode::None => return None,
                BorderMode::Zero => {
                    grid = false;
                    bank = false;
                }
                _ => {}
            }
            offset += borders.offset_u;
        }
        let (iu, clamped) = split(&mut fu, u.n);
        if clamped {
            su = 0.0;
        }

        let v = &self.v;
        let mut mode_v = borders.v;
        let mut in_v = true;
        let (iv, fv, sv);
        if let Some(inc) = v.uniform_inc {
            let mut f = (uv.v - v.first) / inc;
            if uv.v < v.first || uv.v > v.last {
                in_v = false;
                match mode_v {
                    BorderMode::None => return None,
                    BorderMode::Zero => grid = false,
                    _ => {}
                }
                offset += borders.offset_v;
            }
            let (i, clamped) = split(&mut f, v.nodes.len());
            (iv, fv, sv) = (i, f, if clamped { 0.0 } else { 1.0 / inc });
        } else {
            let mut pos = uv.v;
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
                        _ => {}
                    }
                }
                offset += borders.offset_v;
            }
            let nodes = &v.nodes;
            let i = nodes[1..nodes.len() - 1].partition_point(|&node| node <= pos);
            let width = nodes[i + 1] - nodes[i];
            let f = (pos - nodes[i]) / width;
            let clamped = f.clamp(0.0, 1.0);
            (iv, fv, sv) = (i, clamped, if clamped == f { 1.0 / width } else { 0.0 });
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
            border,
            offset,
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

    /// `handmade_straight.crg` with `block` inserted before its `$ROAD_CRG` section.
    fn straight_with(block: &str) -> Vec<u8> {
        let text = String::from_utf8(fixture("handmade_straight.crg")).unwrap();
        let at = text.find("$ROAD_CRG ").unwrap();
        format!("{}{block}\n$\n{}", &text[..at], &text[at..]).into_bytes()
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
    fn unimplemented_features_are_named() {
        let unsupported = |block: &str| match CrgGrid::from_bytes(&straight_with(block)) {
            Err(Error::Unsupported(feature)) => feature,
            other => panic!("{block}: {other:?}"),
        };
        assert_eq!(
            unsupported("$ROAD_CRG_OPTS\nborder_mode_v = 3"),
            "repeat and reflect border modes"
        );
        assert_eq!(
            unsupported("$ROAD_CRG_OPTS\nborder_smooth_ubeg = 1"),
            "border smoothing"
        );
        assert_eq!(
            unsupported("$ROAD_CRG_MODS\nscale_z_grid = 2"),
            "scaling modifiers"
        );
    }

    #[test]
    fn nan_queries_return_none() {
        let grid = CrgGrid::from_bytes(&fixture("handmade_straight.crg")).unwrap();
        assert_eq!(grid.elevation_at_uv(uv(f64::NAN, 0.0)), None);
        assert_eq!(grid.elevation_at_uv(uv(1.0, f64::NAN)), None);
    }
}
