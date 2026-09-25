# opencrg

A pure-Rust reader and evaluator for [ASAM OpenCRG](https://www.asam.net/standards/detail/opencrg/) road surface files (`.crg`). It has no dependencies, reads ASCII and binary files, and matches the ASAM OpenCRG 2.0 C-API to the last bit on the test fixtures.

```rust,no_run
use opencrg::{CrgGrid, SearchHint, Uv, Xy};

fn main() -> Result<(), opencrg::Error> {
    let grid = CrgGrid::from_path("belgian_block.crg")?;
    let z = grid.elevation_at_uv(Uv { u: 12.0, v: 0.4 });
    let normal = grid.normal_at_uv(Uv { u: 12.0, v: 0.4 });

    // A moving point: each search starts where the previous one ended.
    let mut hint = SearchHint::default();
    let mut uv = None;
    for step in 0..100 {
        let xy = Xy { x: 3.0 + 0.1 * f64::from(step), y: 1.0 };
        uv = grid.uv_from_xy_near(xy, &mut hint);
    }
    println!("{z:?} {normal:?} {uv:?}");
    Ok(())
}
```

Requires Rust 1.85 or later.

## API

Loading:

| Function | Includes (`$ROAD_CRG_FILE`) |
|----------|-----------------------------|
| `from_bytes`, `from_bytes_with_options` | Error: `Error::IncludeUnsupported` |
| `from_bytes_with(bytes, options, loader)` | The loader closure gets each name as written |
| `from_path`, `from_path_with_options` | Read from disk, relative to the including file |

`LoadOptions` sets border modes, border offsets, and smoothing zones. Settings in the file's `$ROAD_CRG_OPTS` block take precedence, as in the C-API.

Queries. Grid coordinates are `Uv`: u along the reference line, v to its left, in metres. Global coordinates are `Xy`.

| Method | Returns |
|--------|---------|
| `elevation_at_uv` | Elevation including reference-line height, bank, and placement. `None` outside the grid with border mode `None`, or in a NaN hole |
| `normal_at_uv` | Unit surface normal in the global frame. Also `None` where the grid folds over itself on a tight curve |
| `grid_at_uv` | Grid elevation and slopes only, without reference-line height, bank, or placement shift |
| `heading_at_uv` | Reference-line heading and curvature of the parallel line through the point |
| `xy_from_uv` | Global position |
| `uv_from_xy` | Grid position, searching from scratch |
| `uv_from_xy_near` | Grid position for a moving point. A `SearchHint` carries each search's end to the next query |

`Some` values are always finite. `z_values`, `dims`, `z_shift`, `u_range`, and `v_range` expose the raw grid, for example to build a texture without copying.

`CrgGrid` is `Send` and `Sync` and has no internal state, so one grid can serve many threads.

## Supported features

The crate reads:

- ASCII and binary payloads, in single and double precision.
- Straight reference lines, and reference lines from a heading channel.
- Slope and bank channels.
- Evenly spaced and positioned long sections.
- All five border modes, with offsets.
- Smoothing zones.
- `$ROAD_CRG_MODS` placement, scaling, and NaN handling.
- Includes.

Two features load with `Error::Unsupported`:

- Reference lines given as x, y, or u data channels.
- `REFLINE_CONTINUATION = 1` on a closed reference line, which continues u around the track.

The C-API's contact-point settings, such as history size and search distances, have no equivalent.

## Differences from the C-API

Where the C-API has a bug or leaves a value undefined, the crate does the following instead. Everything else matches the C-API.

- **An included file loads as if it were loaded directly.** The C-API drops the included file's `REFERENCE_LINE_END_X/Y/Z` and long-section spacing settings. For `belgian_block.crg` behind a one-line wrapper, the dropped settings move 33 of 278 sampled elevations by up to 2.4 mm.
- **Includes nest up to 8 levels.** The C-API builds a nested include's path by appending it to its parent's, so only one level works. Where levels disagree, the least nested file's options and modifiers apply.
- **Relative include names resolve against the including file**, not the working directory. `from_path` expands `$VARIABLE` as the C-API does and reports a file that includes itself as `Error::IncludeCycle`.
- **Missing core-area headers.** Without `LONG_SECTION_V_RIGHT/LEFT` or `REFERENCE_LINE_END_PHI`, the C-API uses 0. The crate uses the outer long sections and the last heading. No sample file is affected.
- **On a closed reference line, `border_mode_u = Zero` does not change `uv_from_xy`.** The C-API checks the wrong option there and switches to closed-track mode.
- **The xy to uv search stops after one lap** of a closed reference line. The C-API can loop forever.
- **`SearchHint` keeps one entry** in place of the C-API's 50-entry history, with the same 2.2 m cut-off. On a road that overlaps itself, a point more than 2.2 m from the previous query can land on a different branch than the C-API would pick from its older entries.
- **Invalid scale factors fail.** Length or width factors of 0 or less and non-finite factors are `Error::Invalid`. The C-API reports a failed check and evaluates anyway.
- **Ignored, as in effect in the C-API:** `REFERENCE_LINE_OFFSET_*`, and the `REFLINE_SEARCH_*` options in a file.
- **`crgCheck` is not run.** Its curvature test rejects files the C-API evaluates correctly, such as ASAM's `crg_local_curv_test_ok.crg`.

## wasm

The crate needs `std` and builds for `wasm32-unknown-unknown`. There, `from_path` returns `Error::Io`. Load with `from_bytes`, or with `from_bytes_with` for files with includes. Under WASI, `from_path` works in the directories the runtime grants.

Results under wasm match native, except curvature, which differs by up to 1e-10 per metre. Wasm uses Rust's own `sin` and `cos`, and curvature divides small differences of reference-line positions.

## Performance

`cargo bench` on an AMD Ryzen 9 5900HS, pinned to one core with `taskset -c 2`. Times are per query, and each range spans three runs.

| File | Nodes | Load | `elevation_at_uv` | `normal_at_uv` | `xy_from_uv` | `uv_from_xy` | `uv_from_xy_near` |
|------|-------|------|-------------------|----------------|--------------|--------------|-------------------|
| `belgian_block.crg` | 1001 × 341 | 3–5 ms | 19–21 ns | 70–72 ns | 21 ns | 111–113 ns | 47–48 ns |
| `country_road.crg` | 56 897 × 370 | 170–176 ms | 26 ns | 80–82 ns | 26–27 ns | 5.2–5.9 µs | 56–58 ns |
| `crg_refline_Hoki_HoeKi_Grafing.crg` | 881 975 × 3 | 71–76 ms | 20–26 ns | 80–92 ns | 30–31 ns | 108–116 µs | 65–66 ns |

`uv_from_xy` scans every tenth reference-line node, like the C-API with no history, so its time grows with road length. `uv_from_xy_near` walks from the hint. For a vehicle that moves 0.25 m per query, its time does not depend on road length.

The C-API answering the same queries, built with `gcc -O3` and timed the same way:

| File | Load | `crgEvaluv2z` | `crgEvaluv2xy` | `crgEvalxy2uv`, random points | `crgEvalxy2uv` along the road |
|------|------|---------------|----------------|-------------------------------|-------------------------------|
| `belgian_block.crg` | 3 ms | 19–20 ns | 22 ns | 144–148 ns | 16 ns |
| `country_road.crg` | 267–278 ms | 26–45 ns | 27–28 ns | 6.5–7.3 µs | 26–27 ns |
| `crg_refline_Hoki_HoeKi_Grafing.crg` | 122–128 ms | 28–34 ns | 38–40 ns | 224–226 µs | 62–87 ns |

The crate matches or beats the C-API on every query except one. `uv_from_xy_near` is 3 times slower than the C-API on `belgian_block.crg` and 2 times slower on `country_road.crg`. Most of that gap comes from the hint. A `Uv` hint costs an `xy_from_uv` call before the search starts, while the C-API keeps the previous query's position.

## Development

```sh
cargo test                                # unit tests and recorded C-API results
tools/fetch-large-fixtures.sh             # four ASAM sample files, 136 MB, checked by SHA-256
cargo test --release -- --include-ignored # adds the large files
cargo bench
```

`tests/oracle/` holds C-API results for 50 cases of about 2,200 queries each, recorded by `tools/oracle/record.sh` from the C-API at a pinned commit (`tools/oracle/OpenCRG`, a git submodule). Elevations and positions must match within 1e-9 m, heading within 1e-12 relative, and curvature within 1e-9 per metre. To record again:

```sh
git submodule update --init
cmake -S tools/oracle -B target/oracle -DCMAKE_BUILD_TYPE=Release
cmake --build target/oracle
tools/oracle/record.sh
```

The test fixtures come from ASAM OpenCRG. See `tests/fixtures/NOTICE`.

## Licence

Licensed under either the Apache License 2.0 (`LICENSE-APACHE`) or the MIT licence (`LICENSE-MIT`), at your option.
