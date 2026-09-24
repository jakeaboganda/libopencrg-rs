//! Compares evaluation with C-API results recorded by `tools/oracle/record.sh`.

use std::fs;
use std::path::{Path, PathBuf};

use opencrg::{BorderMode, CrgGrid, LoadOptions, Uv, Xy};

/// Absolute tolerance for elevations and positions, in metres.
const TOLERANCE: f64 = 1e-9;
/// Relative tolerance for heading and curvature.
const RELATIVE: f64 = 1e-12;

fn close(got: f64, want: f64, tolerance: f64) -> bool {
    got == want || (got - want).abs() <= tolerance
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn border_mode(code: &str) -> BorderMode {
    match code {
        "0" => BorderMode::None,
        "1" => BorderMode::Zero,
        "2" => BorderMode::Keep,
        "3" => BorderMode::Repeat,
        "4" => BorderMode::Reflect,
        _ => panic!("bad border mode {code}"),
    }
}

fn numbers(text: &str) -> Vec<f64> {
    text.split_whitespace()
        .map(|n| n.parse().unwrap_or_else(|_| panic!("bad number {n}")))
        .collect()
}

/// Checks one recorded case and returns its mismatches.
fn check_case(path: &Path) -> Vec<String> {
    let text = fs::read_to_string(path).unwrap();
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    let fixture = lines.next().unwrap().strip_prefix("fixture ").unwrap();
    let bytes = fs::read(root().join("tests/fixtures").join(fixture))
        .unwrap_or_else(|e| panic!("{fixture}: {e}"));

    let mut options = LoadOptions::default();
    let mut failures = Vec::new();
    let mut grid = None;
    // The previous uv result since the last reset, which the C-API's search starts from.
    let mut hint = None;
    for line in lines {
        if let Some(option) = line.strip_prefix("option ") {
            let (key, value) = option.split_once('=').unwrap();
            match key {
                "border_mode_u" => options.border_mode_u = border_mode(value),
                "border_mode_v" => options.border_mode_v = border_mode(value),
                "border_offset_u" => options.border_offset_u = value.parse().unwrap(),
                "border_offset_v" => options.border_offset_v = value.parse().unwrap(),
                _ => panic!("unknown option {key}"),
            }
            continue;
        }
        let grid = grid.get_or_insert_with(|| {
            CrgGrid::from_bytes_with_options(&bytes, &options)
                .unwrap_or_else(|e| panic!("{fixture}: {e}"))
        });
        let (query, expected) = line.split_once(" = ").unwrap();
        let (command, args) = query.split_once(' ').unwrap_or((query, ""));
        let args = numbers(args);
        match command {
            "range" => {
                let (u, v) = (grid.u_range(), grid.v_range());
                let expected = numbers(expected);
                if [u.0, u.1, v.0, v.1] != expected[..] {
                    failures.push(format!("{query}: got {u:?} {v:?}, want {expected:?}"));
                }
            }
            "z" => {
                let got = grid.elevation_at_uv(Uv {
                    u: args[0],
                    v: args[1],
                });
                // The C-API reports NaN holes as success; the crate returns None.
                let want = match expected {
                    "none" => None,
                    _ => Some(expected.parse::<f64>().unwrap()).filter(|z| !z.is_nan()),
                };
                let ok = match (got, want) {
                    (Some(got), Some(want)) => close(got, want, TOLERANCE),
                    (None, None) => true,
                    _ => false,
                };
                if !ok {
                    failures.push(format!("{query}: got {got:?}, want {want:?}"));
                }
            }
            "xy" => {
                let got = grid.xy_from_uv(Uv {
                    u: args[0],
                    v: args[1],
                });
                let want = numbers(expected);
                if !(close(got.x, want[0], TOLERANCE) && close(got.y, want[1], TOLERANCE)) {
                    failures.push(format!("{query}: got {got:?}, want {want:?}"));
                }
            }
            "pk" => {
                let got = grid.heading_at_uv(Uv {
                    u: args[0],
                    v: args[1],
                });
                let want = numbers(expected);
                let relative = |got: f64, want: f64| close(got, want, RELATIVE * want.abs());
                if !(relative(got.phi, want[0]) && relative(got.curvature, want[1])) {
                    failures.push(format!("{query}: got {got:?}, want {want:?}"));
                }
            }
            "reset" => hint = None,
            "uv" => {
                let xy = Xy {
                    x: args[0],
                    y: args[1],
                };
                let got = match hint {
                    None => grid.uv_from_xy(xy),
                    Some(hint) => grid.uv_from_xy_near(xy, hint),
                };
                let want = (expected != "none").then(|| numbers(expected));
                let ok = match (got, &want) {
                    (Some(got), Some(want)) => {
                        close(got.u, want[0], TOLERANCE) && close(got.v, want[1], TOLERANCE)
                    }
                    (None, None) => true,
                    _ => false,
                };
                if !ok {
                    failures.push(format!("{query}: got {got:?}, want {want:?}"));
                }
                hint = got;
            }
            _ => panic!("unknown command {command}"),
        }
    }
    failures
}

fn check_cases(large: bool) {
    let mut report = String::new();
    let mut checked = 0;
    let mut paths: Vec<_> = fs::read_dir(root().join("tests/oracle"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    for path in paths {
        let text = fs::read_to_string(&path).unwrap();
        if text.contains("\nfixture large/") != large {
            continue;
        }
        checked += 1;
        let failures = check_case(&path);
        if !failures.is_empty() {
            let name = path.file_stem().unwrap().to_string_lossy();
            report += &format!("{name}: {} mismatches\n", failures.len());
            for failure in failures.iter().take(5) {
                report += &format!("  {failure}\n");
            }
        }
    }
    assert!(checked > 0, "no recorded cases");
    assert!(report.is_empty(), "\n{report}");
}

#[test]
fn recorded_cases() {
    check_cases(false);
}

/// Run `tools/fetch-large-fixtures.sh` first.
#[test]
#[ignore]
fn recorded_large_cases() {
    check_cases(true);
}
