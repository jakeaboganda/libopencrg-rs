use crate::types::BorderMode;

/// `$ROAD_CRG` values. `None` means the key is absent; the C-API's fallbacks apply later.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Road {
    pub start_u: Option<f64>,
    pub start_x: Option<f64>,
    pub start_y: Option<f64>,
    pub start_z: Option<f64>,
    pub start_phi: Option<f64>,
    pub start_bank: Option<f64>,
    pub start_slope: Option<f64>,
    pub end_u: Option<f64>,
    pub end_x: Option<f64>,
    pub end_y: Option<f64>,
    pub end_z: Option<f64>,
    pub end_phi: Option<f64>,
    pub end_bank: Option<f64>,
    pub end_slope: Option<f64>,
    pub u_increment: Option<f64>,
    pub v_right: Option<f64>,
    pub v_left: Option<f64>,
    pub v_increment: Option<f64>,
    pub offset_x: Option<f64>,
    pub offset_y: Option<f64>,
    pub offset_z: Option<f64>,
    pub offset_phi: Option<f64>,
}

/// `$ROAD_CRG_OPTS` values that affect evaluation. Log and warning settings are dropped.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Options {
    pub border_mode_u: Option<BorderMode>,
    pub border_mode_v: Option<BorderMode>,
    pub border_offset_u: Option<f64>,
    pub border_offset_v: Option<f64>,
    pub smooth_u_begin: Option<f64>,
    pub smooth_u_end: Option<f64>,
    /// `true` for `REFLINE_CONTINUATION = 1` (close track).
    pub refline_close_track: Option<bool>,
    pub refline_search_far: Option<f64>,
    pub refline_search_close: Option<f64>,
    pub refline_search_u: Option<f64>,
    pub refline_search_u_fraction: Option<f64>,
}

/// Treatment of NaN grid values, from `GRID_NAN_MODE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NanMode {
    Keep,
    SetZero,
    KeepLast,
}

/// `$ROAD_CRG_MODS` values.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Mods {
    pub scale_z_grid: Option<f64>,
    pub scale_slope: Option<f64>,
    pub scale_banking: Option<f64>,
    pub scale_length: Option<f64>,
    pub scale_width: Option<f64>,
    pub scale_curvature: Option<f64>,
    pub grid_nan_mode: Option<NanMode>,
    pub grid_nan_offset: Option<f64>,
    pub refline_rotcenter_x: Option<f64>,
    pub refline_rotcenter_y: Option<f64>,
    pub refline_offset_phi: Option<f64>,
    pub refline_offset_x: Option<f64>,
    pub refline_offset_y: Option<f64>,
    pub refline_offset_z: Option<f64>,
    pub refpoint_u_fraction: Option<f64>,
    pub refpoint_u_offset: Option<f64>,
    pub refpoint_u: Option<f64>,
    pub refpoint_v_fraction: Option<f64>,
    pub refpoint_v_offset: Option<f64>,
    pub refpoint_v: Option<f64>,
    pub refpoint_x: Option<f64>,
    pub refpoint_y: Option<f64>,
    pub refpoint_z: Option<f64>,
    pub refpoint_phi: Option<f64>,
}

/// Why a `key = value` line was rejected.
pub(crate) type LineError = &'static str;

/// Splits `key = value ! comment` into a lowercase key and the value text.
fn split_assignment(line: &str) -> Result<(String, &str), LineError> {
    let (key, value) = line.split_once('=').ok_or("expected `key = value`")?;
    Ok((key.trim().to_ascii_lowercase(), value))
}

fn float(value: &str) -> Result<f64, LineError> {
    c_atof(value).ok_or("expected a number")
}

fn int(value: &str) -> Result<i64, LineError> {
    c_atoi(value).ok_or("expected an integer")
}

impl Road {
    /// Applies one `$ROAD_CRG` line. Unknown keys are ignored, as in the C-API.
    pub fn apply(&mut self, line: &str) -> Result<(), LineError> {
        let (key, value) = split_assignment(line)?;
        let slot = match key.as_str() {
            "reference_line_start_u" => &mut self.start_u,
            "reference_line_start_x" => &mut self.start_x,
            "reference_line_start_y" => &mut self.start_y,
            "reference_line_start_z" => &mut self.start_z,
            "reference_line_start_phi" => &mut self.start_phi,
            "reference_line_start_b" => &mut self.start_bank,
            "reference_line_start_s" => &mut self.start_slope,
            "reference_line_end_u" => &mut self.end_u,
            "reference_line_end_x" => &mut self.end_x,
            "reference_line_end_y" => &mut self.end_y,
            "reference_line_end_z" => &mut self.end_z,
            "reference_line_end_phi" => &mut self.end_phi,
            "reference_line_end_b" => &mut self.end_bank,
            "reference_line_end_s" => &mut self.end_slope,
            "reference_line_increment" => &mut self.u_increment,
            "long_section_v_right" => &mut self.v_right,
            "long_section_v_left" => &mut self.v_left,
            "long_section_v_increment" => &mut self.v_increment,
            "reference_line_offset_x" => &mut self.offset_x,
            "reference_line_offset_y" => &mut self.offset_y,
            "reference_line_offset_z" => &mut self.offset_z,
            "reference_line_offset_phi" => &mut self.offset_phi,
            _ => return Ok(()),
        };
        *slot = Some(float(value)?);
        Ok(())
    }
}

impl Options {
    /// Applies one `$ROAD_CRG_OPTS` line. Unknown keys are ignored, as in the C-API.
    pub fn apply(&mut self, line: &str) -> Result<(), LineError> {
        let (key, value) = split_assignment(line)?;
        let slot = match key.as_str() {
            "border_mode_u" | "border_mode_v" => {
                let mode =
                    BorderMode::from_code(int(value)?).ok_or("border mode must be 0 to 4")?;
                let slot = if key.ends_with('u') {
                    &mut self.border_mode_u
                } else {
                    &mut self.border_mode_v
                };
                *slot = Some(mode);
                return Ok(());
            }
            "refline_continuation" => {
                self.refline_close_track = Some(match int(value)? {
                    0 => false,
                    1 => true,
                    _ => return Err("refline continuation must be 0 or 1"),
                });
                return Ok(());
            }
            "border_offset_u" => &mut self.border_offset_u,
            "border_offset_v" => &mut self.border_offset_v,
            "border_smooth_ubeg" => &mut self.smooth_u_begin,
            "border_smooth_uend" => &mut self.smooth_u_end,
            "refline_search_far" => &mut self.refline_search_far,
            "refline_search_close" => &mut self.refline_search_close,
            "refline_search_u" => &mut self.refline_search_u,
            "refline_search_ufrac" => &mut self.refline_search_u_fraction,
            _ => return Ok(()),
        };
        *slot = Some(float(value)?);
        Ok(())
    }
}

impl Mods {
    /// Applies one `$ROAD_CRG_MODS` line. Unknown keys are ignored, as in the C-API.
    pub fn apply(&mut self, line: &str) -> Result<(), LineError> {
        let (key, value) = split_assignment(line)?;
        let slot = match key.as_str() {
            "grid_nan_mode" => {
                self.grid_nan_mode = Some(match int(value)? {
                    0 => NanMode::Keep,
                    1 => NanMode::SetZero,
                    2 => NanMode::KeepLast,
                    _ => return Err("grid NaN mode must be 0 to 2"),
                });
                return Ok(());
            }
            "scale_z_grid" => &mut self.scale_z_grid,
            "scale_slope" => &mut self.scale_slope,
            "scale_banking" => &mut self.scale_banking,
            "scale_length" => &mut self.scale_length,
            "scale_width" => &mut self.scale_width,
            "scale_curvature" => &mut self.scale_curvature,
            "grid_nan_offset" => &mut self.grid_nan_offset,
            "refline_rotcenter_x" => &mut self.refline_rotcenter_x,
            "refline_rotcenter_y" => &mut self.refline_rotcenter_y,
            "refline_offset_phi" => &mut self.refline_offset_phi,
            "refline_offset_x" => &mut self.refline_offset_x,
            "refline_offset_y" => &mut self.refline_offset_y,
            "refline_offset_z" => &mut self.refline_offset_z,
            "refpoint_u_fraction" => &mut self.refpoint_u_fraction,
            "refpoint_u_offset" => &mut self.refpoint_u_offset,
            "refpoint_u" => &mut self.refpoint_u,
            "refpoint_v_fraction" => &mut self.refpoint_v_fraction,
            "refpoint_v_offset" => &mut self.refpoint_v_offset,
            "refpoint_v" => &mut self.refpoint_v,
            "refpoint_x" => &mut self.refpoint_x,
            "refpoint_y" => &mut self.refpoint_y,
            "refpoint_z" => &mut self.refpoint_z,
            "refpoint_phi" => &mut self.refpoint_phi,
            _ => return Ok(()),
        };
        *slot = Some(float(value)?);
        Ok(())
    }
}

/// C `atof` on the longest numeric prefix, or `None` if there is no digit.
pub(crate) fn c_atof(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let start = b
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(b.len());
    let mut i = start;
    let digits = |i: &mut usize| {
        let from = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - from
    };
    if i < b.len() && matches!(b[i], b'+' | b'-') {
        i += 1;
    }
    let mut mantissa = digits(&mut i);
    if i < b.len() && b[i] == b'.' {
        i += 1;
        mantissa += digits(&mut i);
    }
    if mantissa == 0 {
        return None;
    }
    if i < b.len() && matches!(b[i], b'e' | b'E') {
        let mut j = i + 1;
        if j < b.len() && matches!(b[j], b'+' | b'-') {
            j += 1;
        }
        if digits(&mut j) > 0 {
            i = j;
        }
    }
    s[start..i].parse().ok()
}

/// C `atoi` on the longest integer prefix, or `None` if there is no digit.
pub(crate) fn c_atoi(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let (negative, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let end = rest
        .bytes()
        .position(|c| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let magnitude = rest[..end].parse::<i64>().unwrap_or(i64::MAX);
    Some(if negative { -magnitude } else { magnitude })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atof_takes_numeric_prefix() {
        assert_eq!(c_atof("-1.50           ! comment"), Some(-1.5));
        assert_eq!(c_atof(" 7.3000000000000000e+002"), Some(730.0));
        assert_eq!(c_atof("1.e5x"), Some(1e5));
        assert_eq!(c_atof(".5"), Some(0.5));
        assert_eq!(c_atof("2e"), Some(2.0));
        assert_eq!(c_atof("1.0D+01"), Some(1.0));
        assert_eq!(c_atof("   "), None);
        assert_eq!(c_atof("-."), None);
    }

    #[test]
    fn atoi_truncates() {
        assert_eq!(c_atoi("   0.0000000000000000e+00"), Some(0));
        assert_eq!(c_atoi(" 2 ! keep"), Some(2));
        assert_eq!(c_atoi("-3.9"), Some(-3));
        assert_eq!(c_atoi("x"), None);
    }

    #[test]
    fn options_decode_c_style_integers() {
        let mut o = Options::default();
        o.apply("border_mode_v        =   0.0000000000000000e+00")
            .unwrap();
        o.apply("  BORDER_MODE_U = 3             ! repeat").unwrap();
        o.apply("REFLINE_CONTINUATION = 1").unwrap();
        o.apply("LOG_EVAL = 20").unwrap();
        assert_eq!(o.border_mode_v, Some(BorderMode::None));
        assert_eq!(o.border_mode_u, Some(BorderMode::Repeat));
        assert_eq!(o.refline_close_track, Some(true));
        assert!(o.apply("BORDER_MODE_U = 7").is_err());
    }
}
