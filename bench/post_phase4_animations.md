# FernUI examples — runtime benchmark

- **Date:** 2026-05-09 18:18:24
- **Duration:** 203.0s
- **Profile:** `release`
- **Warmup:** 5.0s, sampling window: 30.0s
- **GPU probe:** AMD/Intel sysfs (/sys/class/drm/card1/device/gpu_busy_percent)

Memory and CPU are per-process (RSS, sum of children). GPU busy% and VRAM are *system-wide*; the pre-launch baseline is subtracted so the value approximates the example's contribution. Idle GUIs typically show ~0% CPU and ~0% GPU.

## Summary

| Example | Build | Bin size | RSS avg | RSS peak | CPU avg | CPU peak | GPU avg | GPU peak | VRAM Δ | Note |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| `animations` | ok | 14.8 MiB | 113.9 MiB | 114.2 MiB | 5.6% | 11.5% | 2.0% | 3.0% | 7.2 MiB |  |
| `animations-kit` | ok | 14.6 MiB | 112.8 MiB | 113.1 MiB | 4.7% | 11.5% | 0.0% | 0.0% | 10.4 MiB |  |

## Details

### `animations`

- Build: ok (73.5s)
- Binary: `/home/cyril/Devel/fern-ui/target/release/animations`
- Binary size: 14.8 MiB
- Samples collected: 116
- RSS avg / peak: 113.9 MiB / 114.2 MiB
- VMS peak: 1.54 GiB
- CPU avg / peak: 5.6% / 11.5%
- GPU busy avg / peak (Δ vs baseline): 2.0% / 3.0%
- VRAM Δ: 7.2 MiB
- Exit code: -15

### `animations-kit`

- Build: ok (58.5s)
- Binary: `/home/cyril/Devel/fern-ui/target/release/animations-kit`
- Binary size: 14.6 MiB
- Samples collected: 116
- RSS avg / peak: 112.8 MiB / 113.1 MiB
- VMS peak: 1.54 GiB
- CPU avg / peak: 4.7% / 11.5%
- GPU busy avg / peak (Δ vs baseline): 0.0% / 0.0%
- VRAM Δ: 10.4 MiB
- Exit code: -15
