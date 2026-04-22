# Phase 4b Plan — HDetector native Rust port

**Module:** medusa-threed-rs (consumer) + medusa-modules/hdetector-rs (new crate)
**Author:** medusa-modules agent
**Date:** 2026-04-22
**Branch:** docs/phase-4b-plan
**Status:** planning draft — awaiting orchestrator + core sign-off
**Supersedes (in part):** §3.2 and §Phase 4b of MIGRATION_PLAN_3D.md

---

## 0. Context

Phase 3 delivered the gRPC split: `medusa-threed-rs` owns `ThreeDScanService`
(ScanAll / ScanOne / ScaleConfiguration) and calls back into Java's
`MetrologyCallbackService` on :50051 to run `HDetector` (laser line profile
extraction). The callback was always documented as an interim bridge — Phase
4b removes it by porting HDetector to Rust natively.

Phase 4b does **not** touch UI, ScanService request/response shapes, or the
polar→cartesian stage already in Rust. It replaces one internal call.

---

## 1. Architecture — crate, not service

HDetector ships as a **Cargo library crate** inside this repository
(`medusa-modules/hdetector-rs`), consumed by `medusa-threed-rs` as a path /
workspace dependency. It is **not** a gRPC service.

Rationale:
- Single-process call, hot path (one invocation per slice × N slices per
  scan). Network boundary would reintroduce the latency the port is meant to
  eliminate.
- No other in-tree consumer needs it over the wire today. If one appears
  later, wrapping the crate in a service is additive and cheap.
- Keeps the crate testable in isolation with the golden-file harness (§4)
  without standing up a server.

Proposed layout:

```
medusa-modules/
├── medusa-threed-rs/
│   └── Cargo.toml               # depends on hdetector-rs = { path = "../hdetector-rs" }
└── hdetector-rs/                # new crate
    ├── Cargo.toml
    ├── src/
    │   ├── lib.rs               # pub fn detect(mat, decimation, strategy) -> Profile
    │   ├── strategy.rs          # FinderStrategy enum
    │   ├── profile.rs           # Profile struct (left/right/step/scan_area)
    │   ├── image.rs             # imread + RGB2GRAY wrapper
    │   ├── contours.rs          # threshold + findContours pipeline
    │   ├── smoothing.rs         # Akima spline + Fourier low-pass
    │   └── ...
    ├── tests/
    │   └── golden_parity.rs     # §4 harness entry point
    └── goldens/                 # committed fixture profiles
```

A workspace `Cargo.toml` at repo root gets added if not yet present.

---

## 2. Inventory — OpenCV surface used by HDetector

Captured from the Java side (`com.farisland.farvision.libraries.metrology`)
during Phase 3 callback wiring. This is the **only** OpenCV surface
Phase 4b must reproduce:

| Java call | Purpose | Rust equivalent (`opencv` crate v0.93) |
|-----------|---------|----------------------------------------|
| `Imgcodecs.imread(path, flags)` | load PNG slice | `opencv::imgcodecs::imread(path, flags)` |
| `Imgproc.cvtColor(src, dst, COLOR_RGB2GRAY)` | grayscale conversion | `opencv::imgproc::cvt_color(..., COLOR_RGB2GRAY, 0)` |
| `Imgproc.threshold(src, dst, t, maxv, type)` with `THRESH_BINARY` in `BLACK_ON_WHITE` / `WHITE_ON_BLACK` modes | binarize for profile extraction | `opencv::imgproc::threshold(...)` |
| `Imgproc.findContours(img, contours, hierarchy, mode, method)` | extract laser line contour | `opencv::imgproc::find_contours(...)` |
| `Mat.rows() / cols() / channels() / type()` metadata | iterate profile | `Mat::rows()`, `cols()`, `channels()`, `typ()` |
| `Mat.get(row, col, data[])` / `Mat.put(...)` byte-level pixel access | per-row column scan | `Mat::at_2d::<u8>(row, col)` or `Mat::data_bytes()` for bulk |

Notes:
- `opencv-rust` binds the same OpenCV C++ runtime already used by the Java
  side (L4T 36 Orin images ship libopencv — see §8). No new native dep
  beyond what the box already carries.
- `FinderStrategy` maps 1:1 to `THRESH_BINARY` / `THRESH_BINARY_INV` plus the
  orientation flag used when sweeping rows.
- No OpenCV drawing, ML, or DNN modules are touched. Crate features are
  pinned minimally: `features = ["imgcodecs", "imgproc"]`.

---

## 3. Non-OpenCV deps — smoothing pipeline

After contour extraction, HDetector runs two smoothing stages on the raw
profile before returning it:

| Java dep | Purpose | Rust replacement |
|----------|---------|------------------|
| Apache Commons Math3 `AkimaSplineInterpolator` | monotone-ish interpolation across gaps in the detected profile | `splines` crate (Akima variant) — or hand-rolled Akima on `ndarray` if crate's API is too generic |
| Custom Fourier low-pass filter (in-house, written against Commons Math3 FFT) | attenuate per-row jitter | `rustfft` (`RealFftPlanner`) + hand-rolled band mask (the filter itself is ~30 lines) |

Both are pure numeric code, no OpenCV interaction. They run on the profile
arrays produced by §2, so they are trivially unit-testable in isolation
against the Java output (§4 covers this).

Open question for review: whether to pull `splines` or reimplement Akima
directly. Akima is ~80 lines and having it in-tree avoids a transitive
version churn. **Default: reimplement**, switch to the crate only if a
reviewer pushes back.

---

## 4. Parity gate — golden-file harness

A Rust port of a numeric pipeline is not "done" because it compiles and
returns plausible-looking numbers. It is done when it matches Java bit-close
enough on representative data.

Harness design:
- Capture fixtures on the Windows dev PC (AIStation4070) by running the
  existing Java HDetector over a curated dataset and serializing
  `Profile` as JSON: `{ left_upper: [...], right_lower: [...], step,
  scan_area_height }`.
- Commit fixtures under `hdetector-rs/goldens/<dataset>/<slice>.json`
  alongside the source PNG (or a pointer if size forbids — see below).
- Dataset selection (target ~20 slices):
  - 1 clean centered profile (sanity)
  - 2 BLACK_ON_WHITE and 2 WHITE_ON_BLACK
  - 2 with occlusion gaps (exercise Akima)
  - 2 with high-frequency noise (exercise Fourier)
  - 2 edge-of-frame / partial profiles
  - Remainder: randomly sampled from an in-use dataset
- Tolerance: per-sample `abs(java - rust) <= eps_px` where `eps_px` starts
  at `0.5` (half a pixel) and tightens as we eliminate sources of drift.
  Failing slices report a diff plot path, not just a number.
- Runs in `cargo test --test golden_parity`. Wired into CI so a regression
  blocks merge.

Fixture size: PNGs can be large. Either (a) commit a minimized set
(~20 × ~200 KB ≈ 4 MB, acceptable) or (b) keep them in a `goldens-lfs` sibling
repo and reference by SHA. **Default: commit directly**, revisit if the repo
starts bloating.

---

## 5. ETA

**2–3 weeks elapsed** at normal cadence, assuming no unrelated emergencies.
Rough breakdown:

| Block | Days |
|-------|------|
| Crate scaffold + OpenCV wrappers (§2) | 2 |
| Contour → profile pipeline | 3 |
| Akima port + unit tests | 2 |
| Fourier low-pass port + unit tests | 2 |
| Golden-file harness (§4) infra + fixture capture on AIStation4070 | 2 |
| Tolerance tightening + diff investigation | 3 |
| Integration into `medusa-threed-rs` behind flag (§7) | 2 |
| Buffer / review / rework | 2–5 |

The largest unknown is the tolerance-tightening block. Commons Math3 Akima
and the custom Fourier filter both have accumulated floating-point behavior
that may not match `f64`-exact in Rust; if we hit a stubborn divergence,
dropping `eps_px` from 0.5 to 0.1 could cost another week.

---

## 6. Cutover — `metrology_callback.proto` (Option A)

Per orchestrator decision: **deprecate, do not remove**.

- `metrology_callback.proto` stays in `medusa-protos` and in this repo's
  `proto/` tree.
- Service message gets a `// DEPRECATED: superseded by hdetector-rs crate
  (Phase 4b). Removal targeted Phase 5.` comment.
- `option deprecated = true;` is added on the service and on
  `DetectProfile` for tooling signal.
- Java-side implementation remains compilable and buildable; it is kept as
  an escape hatch behind the feature flag (§7).
- No breaking change to any generated artifact. Downstream consumers that
  never flipped the flag keep working unchanged.

Removal (Phase 5) is a separate ticket gated on: flag defaulting to
`rust_native` in production for ≥2 release cycles with zero parity fallbacks
triggered.

---

## 7. Runtime flag

New config key: `medusa.threed.detector`

| Value | Behavior |
|-------|----------|
| `java_callback` (default for first release) | current Phase 3 path — gRPC to Java `MetrologyCallbackService` |
| `rust_native` | call `hdetector-rs` in-process |

- Read in `ThreeDScanService` at startup; no per-request toggle (keeps hot
  path branch-free).
- Logged prominently at boot so field diagnostics can tell which path ran.
- Flip plan: default switches to `rust_native` once the golden-file harness
  is green in CI for ≥1 week and one full in-house scan session has been run
  on AIStation4070.

No shadow / dual-call mode proposed. Golden-file harness is the parity
signal; adding a runtime shadow call doubles OpenCV load on the Orin for no
new information.

---

## 8. Coordination — sccache-redis + L4T 36 Orin

Already aligned with the toolchain work; this section documents the
dependencies so Phase 4b doesn't rediscover them.

- **sccache-redis**: `opencv-rust` compiles are slow (~5 min cold). The
  shared sccache-redis already caches `medusa-threed-rs` builds; adding
  `hdetector-rs` as a workspace member reuses the same cache keys. No new
  infra needed, but CI workers must have `RUSTC_WRAPPER=sccache` and
  `SCCACHE_REDIS` set — to verify on the runner image before merging.
- **L4T 36 Orin**: target deploys ship `libopencv 4.x` from the L4T apt
  index. The `opencv` crate's `clang-runtime` build needs the matching
  `libopencv-dev` and `libclang`. Confirmed present on AIStation4070 per
  prior thread; to confirm present on the Orin reference image before
  Phase 4b merge. The LIVE-branch `libopencv restart HOLD` from Phase 3
  (see project memory) is the same underlying concern — resolving it
  unblocks Phase 4b deployment, not just LIVE.

---

## 9. Out of scope (explicitly)

- Porting any other metrology class beyond `HDetector` / `Profile` /
  `FinderStrategy`.
- Changing the `ScanAll` / `ScanOne` wire contracts.
- Performance work beyond what a straight port delivers. If `rust_native`
  is slower than `java_callback`, that is a bug; if it is merely
  "not-yet-fast", that is a Phase 5 optimization ticket.
- UI changes. JavaFX stays externally unchanged, per STRATEGY.md.

---

## 10. Sign-off checklist

- [ ] Orchestrator ACK on crate-not-service (§1) and Option A (§6)
- [ ] medusa-core ACK on `metrology_callback.proto` deprecation comment wording
- [ ] Toolchain agent confirms sccache-redis + L4T 36 libopencv on CI runners and Orin image (§8)
- [ ] Fixture capture plan on AIStation4070 scheduled (§4)
- [ ] Flag default + rollback path confirmed with ops (§7)
