# Sensor traces

Recorded lid angle and accelerometer data. The detector tests replay these files (ADR-0010).

| Directory | Format | How it was recorded |
|-----------|--------|---------------------|
| `spike-m4air-01/` | legacy (spike) | `tools/record-spike.sh`: separate `accel.ndjson` / `lid.ndjson` / `labels.ndjson` with unix-ms label times |
| `m4air-v1/` | **v1** (below) | `tools/trace-recorder --scenario all` on a MacBook Air 15" M4, macOS 26.6 |

Record new traces on a real MacBook with:

```bash
cargo run --release -p trace-recorder -- --scenario all --out fixtures/traces/<machine>-v1
```

Run `--list` to see the available scenarios.

## Format v1

Each scenario produces two files.

**`<scenario>.ndjson`**: the first line is a header, and every following line is one sample, sorted by time.

```json
{"format":"spank-trace","version":1,"scenario":"baseline","model":"Mac16,13","os":"26.6.2 (25G83)","t0_unix_ms":1792800000000.0,"accel_interval_us":1000,"lid_poll_hz":30,"accel_dropped":0}
{"t":0.50012,"a":[0.01960,0.07060,-0.99190]}
{"t":0.50210,"lid":108}
```

- `t`: seconds since the session start (`t0_unix_ms`).
- `a`: raw accelerometer x/y/z in g, at about 800 Hz. Gravity is still included.
- `lid`: lid angle in whole degrees, polled at 30 Hz.

**`<scenario>.labels.json`**: the labeled segments, using the same time base. Segments are back-to-back.

```json
{ "labels": [ { "label": "slap_hard", "start": 25.012, "end": 33.015 } ] }
```

The label names are defined in `tools/trace-recorder/src/main.rs` (`scenarios()`):
- `slap_*`: the user slapped the laptop.
- `playback_*`: a system sound was looping through the speakers.
- Every other label is a non-slap activity, and the detector must not fire during it.
