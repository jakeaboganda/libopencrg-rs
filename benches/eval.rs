//! Load and query timings: `cargo bench`. Uses the large ASAM samples when
//! `tools/fetch-large-fixtures.sh` has downloaded them.

use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use opencrg::{CrgGrid, Uv};

const FIXTURES: [&str; 4] = [
    "crg-txt/handmade_curved_banked_sloped.crg",
    "crg-bin/belgian_block.crg",
    "large/country_road.crg",
    "large/crg_refline_Hoki_HoeKi_Grafing.crg",
];

const QUERIES: usize = 10_000;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for fixture in FIXTURES {
        let path = root.join(fixture);
        if !path.exists() {
            println!("{fixture}: missing, skipped");
            continue;
        }
        let (grid, load) = time_once(|| CrgGrid::from_path(&path).unwrap());
        let (nu, nv) = grid.dims();
        println!("{fixture} ({nu} x {nv} nodes), load {load:.1?}");

        let points = points(&grid);
        let xys: Vec<_> = points.iter().map(|&uv| grid.xy_from_uv(uv)).collect();
        report("elevation_at_uv", || {
            for &uv in &points {
                black_box(grid.elevation_at_uv(black_box(uv)));
            }
        });
        report("normal_at_uv", || {
            for &uv in &points {
                black_box(grid.normal_at_uv(black_box(uv)));
            }
        });
        report("xy_from_uv", || {
            for &uv in &points {
                black_box(grid.xy_from_uv(black_box(uv)));
            }
        });
        report("uv_from_xy", || {
            for &xy in &xys {
                black_box(grid.uv_from_xy(black_box(xy)));
            }
        });

        // A vehicle at 25 m/s sampled at 100 Hz: each query starts from the previous answer.
        let (u0, u1) = grid.u_range();
        let track: Vec<_> = (0..QUERIES)
            .map(|i| {
                let u = (u0 + 0.25 * i as f64).min(u1);
                grid.xy_from_uv(Uv { u, v: 0.3 })
            })
            .collect();
        report("uv_from_xy_near along the road", || {
            let mut hint = Uv { u: u0, v: 0.3 };
            for &xy in &track {
                hint = grid.uv_from_xy_near(black_box(xy), hint).unwrap_or(hint);
            }
            black_box(hint);
        });
    }
}

/// Pseudo-random points over the grid and a margin of 10 % around it.
fn points(grid: &CrgGrid) -> Vec<Uv> {
    let ((u0, u1), (v0, v1)) = (grid.u_range(), grid.v_range());
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |lo: f64, hi: f64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let t = (state >> 11) as f64 / (1u64 << 53) as f64;
        let margin = 0.1 * (hi - lo);
        lo - margin + t * (hi - lo + 2.0 * margin)
    };
    (0..QUERIES)
        .map(|_| Uv {
            u: next(u0, u1),
            v: next(v0, v1),
        })
        .collect()
}

fn time_once<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = f();
    (value, start.elapsed())
}

/// Prints the median time per query of `QUERIES` queries over repeated runs.
fn report(name: &str, mut run: impl FnMut()) {
    run();
    let mut samples: Vec<Duration> = (0..11).map(|_| time_once(&mut run).1).collect();
    samples.sort();
    let per_query = samples[samples.len() / 2].as_secs_f64() * 1e9 / QUERIES as f64;
    println!("  {name:<31} {per_query:>8.1} ns");
}
