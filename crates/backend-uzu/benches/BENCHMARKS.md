# Kernel Benchmarks

Criterion-based microbenchmarks for Metal kernels. Runs on macOS (native)
and on iPhone (via `cargo-dinghy`). Results from both are consolidated
under a single `target/criterion/<label>/` tree so you can compare
baselines side by side.

## Prerequisites

- Rust nightly: `rustup toolchain install nightly`
- For iOS runs:
    - Apple target: `rustup target add aarch64-apple-ios`
    - `cargo-dinghy` built from the [trymirai/dinghy](https://github.com/trymirai/dinghy/tree/mirai):

      ```bash
      cargo install \
        --git https://github.com/trymirai/dinghy \
        --branch mirai \
        cargo-dinghy
      ```

    - A physical device connected via USB with a device ID from
      `xcrun devicectl list devices`.

## Available benchmark groups

The Cargo bench target is `main`. Its source lives at
`crates/backend-uzu/benches/main.rs`.

| Group id                                | Filter                              |
|-----------------------------------------|-------------------------------------|
| `Metal/Kernel/Matmul/GEMM`              | `Metal/Kernel/Matmul/GEMM`          |
| `Metal/Kernel/Matmul/GEMM_MXU`          | `Metal/Kernel/Matmul/GEMM_MXU`      |
| `Metal/Kernel/UnifiedQuantizedGemm/...` | `Metal/Kernel/UnifiedQuantizedGemm` |
| `Metal/Kernel/Gemv/...`                 | `Metal/Kernel/Gemv`                 |
| `Metal/Kernel/Qwen3Layers/...`          | `Metal/Kernel/Qwen3Layers`          |
| `Metal/Kernel/RMSNorm`                  | `Metal/Kernel/RMSNorm`              |
| `Metal/Kernel/Sampling/Argmax`          | `Metal/Kernel/Sampling/Argmax`      |
| `ChatSession run`                       | `ChatSession run`                   |
| `ChatSession structured/no_thinking`    | `ChatSession structured/no_thinking`|
| `ChatSession structured/thinking`       | `ChatSession structured/thinking`   |
| `Forward pass`                          | `Forward pass`                      |

The prefix `Metal/Kernel/Matmul` runs both `GEMM` and `GEMM_MXU` in one pass.
The session and language-model groups require the test model path configured by
the test helpers.

## Output layout

Every run writes into `target/criterion/<label>/…`, where `<label>` is a
free-form name you choose (e.g. `m2_max`, `a19`). The Criterion baseline
you saved lives at `target/criterion/<label>/<benchmark-path>/<baseline-name>/`.

## Running on macOS

From the repo root. Use an **absolute** `CRITERION_HOME` so it doesn't
resolve relative to the package dir:

```bash
CRITERION_HOME="$PWD/target/criterion/m2_max" cargo bench \
  -p backend-uzu \
  --bench main -- "Metal/Kernel/Matmul" \
  --save-baseline matmul_baseline_m2_max
```

Set `UZU_CAPTURE_BENCH=1` to capture the first matching benchmark command
buffer as a Metal `.gputrace`. `UZU_CAPTURE_BENCH_FILTER` is an optional
benchmark path substring; `UZU_CAPTURE_BENCH_DIR` defaults to the current
directory.

## Running on iPhone (via `cargo-dinghy`)

Run one benchmark group at a time to avoid the iOS watchdog killing the
app.

Key flags:

- `-e CRITERION_HOME=target/criterion/a19` — on-device env var. Path is
  relative to the app's cwd (`Documents/`), so this becomes
  `Documents/target/criterion/a19/` on device.
- `--copy-back "Documents/target=$(pwd)/target"` — after the run,
  `cargo-dinghy` pulls `Documents/target` from the device into your
  repo's `target/`. `$(pwd)` is required (absolute DST) because the
  cargo runner is launched with cwd set to the package dir, not the
  workspace root.

```bash
DEVICE=<DEVICE_ID>

cargo dinghy \
  -d "$DEVICE" \
  -e CRITERION_HOME=target/criterion/a19 \
  --copy-back "Documents/target=$(pwd)/target" \
  bench -p backend-uzu --bench main -- \
    "Metal/Kernel/Matmul" \
    --save-baseline matmul_baseline_a19
```

After the run completes you'll have
`target/criterion/a19/Metal/Kernel/Matmul/<GEMM|GEMM_MXU>/…/matmul_baseline_a19/`
on the host, next to any `m2_max/` baselines.

## Viewing reports

Open the Criterion HTML report:

```bash
open target/criterion/report/index.html
```

To inspect a specific label only:

```bash
open target/criterion/m2_max/report/index.html
open target/criterion/a19/report/index.html
```

## Structured Output benchmark runbook

The structured benchmark matrix includes:

- `plain_*` baselines
- `plain_forced_sync_*` baselines (`generate_suffix_length > 1`) to isolate sync-loop overhead
- `structured_calendar_event_*` (`JsonSchema`) scenarios
- `structured_builtin_json_*` (`BuiltinJson`) scenarios

Deterministic decode is enforced with greedy sampling and a fixed seed. For
no-thinking runs, `tokens_limit` is `32,128,256,512` by default. Set
`UZU_SO_LONG_LIMIT=1` to include `1024`.

### No-thinking benchmark group

```bash
CRITERION_HOME="$PWD/target/criterion/local_so" cargo bench \
  -p backend-uzu \
  --bench main -- "ChatSession structured/no_thinking"
```

### Thinking benchmark group

Requires a downloaded thinking-capable model:

```bash
THINKING_TEST_MODEL=/absolute/path/to/model \
CRITERION_HOME="$PWD/target/criterion/local_so" cargo bench \
  -p backend-uzu \
  --bench main -- "ChatSession structured/thinking"
```

If `THINKING_TEST_MODEL` is not set, the thinking benchmark group is skipped
with an explicit message.

### Companion performance tests

```bash
cargo test -p backend-uzu --test performance_main -- --ignored structured_output

THINKING_TEST_MODEL=/absolute/path/to/model \
cargo test -p backend-uzu --test performance_main -- --ignored structured_output_thinking
```

These tests print:

- median `total/prefill/generate` latencies per scenario
- slowdown ratios:
  - `structured/plain_async_candidate`
  - `structured/plain_forced_sync`
  - `builtin_json/plain_forced_sync`
- cold-vs-warm compile proxy to estimate one-off grammar setup overhead

Ratio interpretation:

- `<= 5%`: noise / neutral
- `5-10%`: noticeable, rerun for confirmation
- `> 10%`: material improvement/regression
