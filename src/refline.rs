//! The reference line in the global frame: node positions from integrated headings
//! (`calcRefLine`, crgLoader.c:2109), uv to xy (crgEvaluv2xy.c:55), and heading and
//! curvature (crgEvalpk.c), and xy to uv (crgEvalxy2uv.c).

use crate::grid::UAxis;
use crate::parse::Road;
use crate::{Heading, Uv, Xy};

#[derive(Clone, Debug)]
pub(crate) struct RefLine {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    /// Heading per cross section; empty for a straight line without a heading channel.
    pub phi: Vec<f64>,
    pub first: End,
    pub last: End,
    /// Whether the C-API treats the line as possibly closed, which lets the xy to uv search
    /// wrap from one end to the other.
    pub closed: bool,
}

/// Position and heading used beyond either end of the reference line.
#[derive(Clone, Copy, Debug)]
pub(crate) struct End {
    pub x: f64,
    pub y: f64,
    pub phi: f64,
    pub sin: f64,
    pub cos: f64,
}

impl End {
    fn new(x: f64, y: f64, phi: f64) -> Self {
        Self {
            x,
            y,
            phi,
            sin: phi.sin(),
            cos: phi.cos(),
        }
    }
}

impl RefLine {
    pub fn new(road: &Road, u: &UAxis, phi: Option<Vec<f64>>) -> Self {
        let n = u.n;
        let inc = u.inc;
        let (x0, y0) = (road.start_x.unwrap_or(0.0), road.start_y.unwrap_or(0.0));
        let phi0 = road.start_phi.unwrap_or(0.0);
        let end = road.end_x.zip(road.end_y);
        let mut x = vec![0.0; n];
        let mut y = vec![0.0; n];

        let phi = match phi {
            None => {
                let (sin, cos) = phi0.sin_cos();
                for i in 0..n {
                    x[i] = x0 + i as f64 * inc * cos;
                    y[i] = y0 + i as f64 * inc * sin;
                }
                Vec::new()
            }
            Some(phi) => {
                if let Some((ex, ey)) = end {
                    // Integrate back from the end, then forward from the start, blending
                    // toward the backward result.
                    x[n - 1] = ex;
                    y[n - 1] = ey;
                    for i in (1..n).rev() {
                        x[i - 1] = x[i] - inc * phi[i].cos();
                        y[i - 1] = y[i] - inc * phi[i].sin();
                    }
                    x[0] = x0;
                    y[0] = y0;
                    for i in 0..n - 1 {
                        let fraction = (i + 1) as f64 / (n - 1) as f64;
                        x[i + 1] = (1.0 - fraction) * (x[i] + inc * phi[i + 1].cos())
                            + fraction * x[i + 1];
                        y[i + 1] = (1.0 - fraction) * (y[i] + inc * phi[i + 1].sin())
                            + fraction * y[i + 1];
                    }
                } else {
                    x[0] = x0;
                    y[0] = y0;
                    for i in 0..n - 1 {
                        x[i + 1] = x[i] + inc * phi[i + 1].cos();
                        y[i + 1] = y[i] + inc * phi[i + 1].sin();
                    }
                }
                phi
            }
        };

        let (x1, y1) = end.unwrap_or((x[n - 1], y[n - 1]));
        // The C-API uses 0 when REFERENCE_LINE_END_PHI is missing; the last heading is used
        // instead.
        let phi1 = road
            .end_phi
            .unwrap_or_else(|| phi.last().copied().unwrap_or(phi0));
        let mut line = RefLine {
            first: End::new(x0, y0, phi0),
            last: End::new(x1, y1, phi1),
            x,
            y,
            phi,
            closed: false,
        };
        line.closed = line.is_closed();
        line
    }

    /// The closed-line test of `crgCalcUtilityData` (crgStatistics.c:238): the end
    /// directions are less than 60 degrees apart and each end lies behind the other.
    fn is_closed(&self) -> bool {
        let (a, b) = (&self.first, &self.last);
        let divisor = a.cos * b.cos + a.sin * b.sin;
        if divisor <= 0.5 {
            return false;
        }
        let l = ((a.y - b.y) * a.sin + (a.x - b.x) * a.cos) / divisor;
        let k = ((b.y - a.y) * b.sin + (b.x - a.x) * b.cos) / divisor;
        l > 0.0 && k < 0.0
    }

    /// Rotates by `angle` around `center`, then translates by `shift`, as
    /// `crgDataApplyTransformations` does (crgMgr.c:928-960).
    pub fn transform(&mut self, center: Xy, angle: f64, shift: Xy) {
        self.phi.iter_mut().for_each(|phi| *phi += angle);
        for end in [&mut self.first, &mut self.last] {
            let (mut x, mut y) = (end.x, end.y);
            rotate(&mut x, &mut y, center, angle);
            *end = End::new(x + shift.x, y + shift.y, end.phi + angle);
        }
        for (x, y) in self.x.iter_mut().zip(&mut self.y) {
            rotate(x, y, center, angle);
            *x += shift.x;
            *y += shift.y;
        }
        self.closed = self.is_closed();
    }

    /// Search start for a point with no usable hint: the nearest of every tenth node and the
    /// last node.
    pub fn coarse_index(&self, xy: Xy) -> usize {
        let n = self.x.len();
        let (mut best, mut best_dist2) = (0, 0.0);
        let mut i = 0;
        loop {
            let (dx, dy) = (xy.x - self.x[i], xy.y - self.y[i]);
            let dist2 = dx * dx + dy * dy;
            if dist2 < best_dist2 || i == 0 {
                best = i;
                best_dist2 = dist2;
            }
            if i + 10 < n {
                i += 10;
            } else if i < n - 1 {
                i = n - 1;
            } else {
                return best;
            }
        }
    }

    /// Grid position of a global position, searching from node `start` (crgEvalxy2uv.c:48).
    pub fn uv(&self, u: &UAxis, xy: Xy, start: usize) -> Uv {
        let (x, y) = (xy.x, xy.y);
        let (px, py) = (&self.x, &self.y);
        let n = px.len();
        let mut index = start.max(1);

        // Walk up while P lies ahead of the normal through node `index`.
        let mut wrap_dot = 0.0;
        let mut wrapped = false;
        loop {
            let next = (index + 1).min(n - 1);
            let dot = (x - px[index]) * (px[next] - px[index - 1])
                + (y - py[index]) * (py[next] - py[index - 1]);
            if dot <= 0.0 || dot.is_nan() {
                break;
            }
            if index < n - 1 {
                index += 1;
            } else if self.closed && !wrapped {
                wrap_dot = (x - px[0]) * (px[1] - px[0]) + (y - py[0]) * (py[1] - py[0]);
                if wrap_dot <= 0.0 {
                    break;
                }
                // The C-API would walk round again forever; one wrap is enough.
                wrapped = true;
                index = 1;
            } else {
                break;
            }
        }

        // Then walk down until P lies ahead of the normal through node `index - 1`.
        let (mut p0, mut p1, mut p2);
        let mut dot;
        loop {
            let i0 = index.saturating_sub(2);
            p0 = [px[i0], py[i0]];
            p1 = [px[index - 1], py[index - 1]];
            p2 = [px[index], py[index]];
            dot = (x - p1[0]) * (p2[0] - p0[0]) + (y - p1[1]) * (p2[1] - p0[1]);
            if dot >= 0.0 || dot.is_nan() {
                break;
            }
            if index > 1 {
                index -= 1;
            } else if self.closed {
                if wrap_dot != 0.0 {
                    break;
                }
                let last = n - 1;
                wrap_dot = (x - px[last]) * (px[last] - px[last - 1])
                    + (y - py[last]) * (py[last] - py[last - 1]);
                if wrap_dot >= 0.0 {
                    break;
                }
                index = last;
            } else {
                break;
            }
        }

        // v is the signed distance from P1P2; u interpolates between the mitred normals
        // through P1 and P2.
        let d21 = [p2[0] - p1[0], p2[1] - p1[1]];
        let d1 = [x - p1[0], y - p1[1]];
        let v = (d21[0] * d1[1] - d21[1] * d1[0]) / (d21[0] * d21[0] + d21[1] * d21[1]).sqrt();
        let i3 = (index + 1).min(n - 1);
        let d31 = [px[i3] - p1[0], py[i3] - p1[1]];
        let d20 = [p2[0] - p0[0], p2[1] - p0[1]];
        let ta = dot / (d20[0] * d21[0] + d20[1] * d21[1]);
        let tb =
            (d31[0] * (p2[0] - x) + d31[1] * (p2[1] - y)) / (d31[0] * d21[0] + d31[1] * d21[1]);
        let du = ta / (ta + tb) * u.inc;
        let at = (index - 1) as f64 * u.inc + du + u.first;

        let end = if at < u.first {
            (&self.first, u.first)
        } else if at > u.last {
            (&self.last, u.last)
        } else {
            return Uv { u: at, v };
        };
        let (e, u0) = end;
        let (dx, dy) = (x - e.x, y - e.y);
        Uv {
            u: u0 + dx * e.cos + dy * e.sin,
            v: dy * e.cos - dx * e.sin,
        }
    }

    /// Global position of a grid position (crgEvaluv2xy.c:55).
    pub fn xy(&self, u: &UAxis, uv: Uv) -> Xy {
        let v = uv.v;
        let mut frac = (uv.u - u.first) / u.inc;
        let index = if frac < 0.0 {
            0
        } else {
            (frac as usize).min(u.n - 2)
        };
        frac -= index as f64;

        if frac < 0.0 {
            let du = frac * u.inc;
            let e = &self.first;
            return Xy {
                x: e.x + du * e.cos - v * e.sin,
                y: e.y + du * e.sin + v * e.cos,
            };
        }
        if frac > 1.0 {
            let du = (frac - 1.0) * u.inc;
            let e = &self.last;
            return Xy {
                x: e.x + du * e.cos - v * e.sin,
                y: e.y + du * e.sin + v * e.cos,
            };
        }

        let p1 = [self.x[index], self.y[index]];
        let p2 = [self.x[index + 1], self.y[index + 1]];
        let n12 = normalize([-(p2[1] - p1[1]), p2[0] - p1[0]]);

        // Offset directions at P1 and P2 bisect the neighbouring segments and are stretched
        // so that the offset line stays parallel to P1P2.
        let mut n1 = n12;
        if index > 0 {
            let p0 = [self.x[index - 1], self.y[index - 1]];
            n1 = normalize([-(p2[1] - p0[1]), p2[0] - p0[0]]);
        }
        let n1 = stretch(n1, n12);
        let a = [p1[0] + v * n1[0], p1[1] + v * n1[1]];

        let mut n2 = n12;
        if index < self.x.len() - 2 {
            let p3 = [self.x[index + 2], self.y[index + 2]];
            n2 = normalize([-(p3[1] - p1[1]), p3[0] - p1[0]]);
        }
        let n2 = stretch(n2, n12);
        let b = [p2[0] + v * n2[0], p2[1] + v * n2[1]];

        let ab = [b[0] - a[0], b[1] - a[1]];
        Xy {
            x: a[0] + frac * ab[0],
            y: a[1] + frac * ab[1],
        }
    }

    /// Heading and curvature (crgEvalpk.c), with curvature taken at offset v as in the
    /// C-API's default lateral curvature mode. Curvature is 0 within 0.5 m of either end.
    pub fn heading(&self, u: &UAxis, uv: Uv) -> Heading {
        let reference = self.reference_heading(u, uv.u);
        let mut curvature = reference.curvature;
        if curvature.abs() > 1.0e-10 {
            let radius = 1.0 / curvature - uv.v;
            curvature = if radius.abs() < 1.0e-6 {
                1.0e6
            } else {
                1.0 / radius
            };
        }
        Heading {
            phi: reference.phi,
            curvature,
        }
    }

    /// Heading and curvature of the reference line itself.
    pub fn reference_heading(&self, u: &UAxis, at: f64) -> Heading {
        let frac = (at - u.first) / u.inc;
        let index = if frac < 0.0 {
            0
        } else {
            (frac as usize).min(u.n - 2)
        };
        // Curvature spans sections at least 0.5 m long.
        let nu = ((0.5 / u.inc) as usize).max(1);

        if nu > index {
            Heading {
                phi: self.first.phi,
                curvature: 0.0,
            }
        } else if index + nu >= self.phi.len() {
            Heading {
                phi: self.last.phi,
                curvature: 0.0,
            }
        } else {
            let hd = 1.0 / (u.inc * nu as f64).powf(3.0);
            let dx0 = self.x[index] - self.x[index - nu];
            let dx1 = self.x[index + nu] - self.x[index];
            let dy0 = self.y[index] - self.y[index - nu];
            let dy1 = self.y[index + nu] - self.y[index];
            Heading {
                phi: self.phi[index + 1],
                curvature: (dx0 * dy1 - dy0 * dx1) * hd,
            }
        }
    }
}

fn normalize(v: [f64; 2]) -> [f64; 2] {
    let length = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if length < 1.0e-10 {
        v
    } else {
        [v[0] / length, v[1] / length]
    }
}

/// Divides `n` by its projection on `n12`.
fn stretch(n: [f64; 2], n12: [f64; 2]) -> [f64; 2] {
    let dot = n[0] * n12[0] + n[1] * n12[1];
    if dot.abs() > 1.0e-10 {
        [n[0] / dot, n[1] / dot]
    } else {
        n
    }
}

/// Rotates a point around `center`, through polar coordinates as the C-API does.
fn rotate(x: &mut f64, y: &mut f64, center: Xy, angle: f64) {
    let dx = *x - center.x;
    let dy = *y - center.y;
    let angle = dy.atan2(dx) + angle;
    let dist = (dx * dx + dy * dy).sqrt();
    *x = center.x + dist * angle.cos();
    *y = center.y + dist * angle.sin();
}
