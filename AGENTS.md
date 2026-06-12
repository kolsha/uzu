# AGENTS.md — Rust guide for the uzu workspace

This file teaches AI agents how to write Rust that matches the existing style of the
`uzu` workspace. It is Rust-only for now (bindings-side Python/Swift/TypeScript style
guides may be added later). Follow it for every Rust change; when in doubt, mirror the
nearest existing module.

## 1. Project overview & crate map

`uzu` is a high-performance, on-device inference engine. The workspace ([Cargo.toml](Cargo.toml))
has these crates under `crates/`:

- Core inference: `backend-uzu` — model config, forward pass, CPU/Metal kernels, sessions.
- Shared types & traits: `shoji` — the public type system (`types::{basic,model,session}`) and backend traits.
- Session layer: `nagare` — chat / classification / TTS streaming sessions, telemetry, API client.
- Chat templates: `hanashi` — Jinja-based chat rendering + OpenAI Harmony encoding.
- SDK surface: `uzu` — `Engine` orchestration, storage, registries, optional TUI, bindings entry points.
- Remote backend: `backend-remote` — OpenAI-compatible remote inference implementing `shoji` backend traits.
- Helpers: `json-transform`, `token-stream-parser`, `download-manager`, `mock-registry`.
- Bindings & macros: `bindings`, `bindings-types`, `proc-macros`.
- Binaries: `cli`, `cli-storage`, `cli-tools`, `benchmarks`.

Crate names follow a Japanese theme: `shoji` = types/traits, `nagare` = sessions/flow,
`hanashi` = chat/talk.

The public `uzu` crate re-exports its dependencies rather than redefining types
([crates/uzu/src/lib.rs](crates/uzu/src/lib.rs)):

```51:52:crates/uzu/src/lib.rs
#[cfg(not(target_family = "wasm"))]
pub use shoji::*;
```

It also aliases `pub use nagare as session;`. So a public type referenced as
`uzu::types::session::chat::ChatConfig` actually lives in `shoji::types`. Put new public
types in `shoji`, new session logic in `nagare`, and only orchestration in `uzu`.

## 2. Toolchain, build & formatting (hard rules)

- Edition `2024`, `rust-version = "1.94"`, `resolver = "3"`, all inherited from the workspace
  (`edition.workspace = true`, etc.). New crates inherit the same way.
- Run `cargo fmt` before finishing. The [rustfmt.toml](rustfmt.toml) settings are mandatory and
  shape how code looks here:
  - `imports_granularity = "Crate"` — merge imports per crate (`use crate::{a, b};`).
  - `group_imports = "StdExternalCrate"` — three import blocks in order: `std`, external crates, then `crate`/`super`.
  - `fn_params_layout = "Vertical"` — each function parameter on its own line (you will see this everywhere; do not collapse them).
  - `match_block_trailing_comma = true`, `use_small_heuristics = "Off"`, `max_width = 120`.
- Lints: every crate opts into the workspace lint table with `[lints] workspace = true`. Add that
  block to any new crate. The allowed clippy lints are in `[workspace.lints.clippy]` (e.g.
  `module_inception`, `type_complexity`, `upper_case_acronyms`, `too_many_arguments`,
  `new_without_default`) — do not re-fight lints that are already allowed there.
- Dependencies live in `[workspace.dependencies]`. Reference them as `dep.workspace = true` (or
  `dep = { workspace = true, features = [...] }`). Never pin a fresh version inside a member crate;
  add or bump it in the workspace table.
- Build/test/run via the `cargo tools` wrapper (`cargo tools build rust --targets apple`,
  `cargo tools test python`, `cargo tools example rust chat`). Fetch the test model with
  `./scripts/download_test_model.sh`.

## 3. Module & file organization

- `mod.rs` lives at directory roots; leaf types/functions go in their own named `.rs` files. Deep
  domains (`backend-uzu/config`, `backend-uzu/backends`, `shoji/types`, `hanashi`) nest 3–5 levels.
- Error types go in a per-module `error.rs` (see section 4).
- Conditional compilation is layered, from coarse to fine:
  - Cargo features: `metal`, `grammar`, `tracing`, `bindings-*`, `capability-*`, `backend-*`.
  - Build-script cfgs emitted by [crates/backend-uzu/build/main.rs](crates/backend-uzu/build/main.rs):

```29:38:crates/backend-uzu/build/main.rs
    let metal_backend = cfg!(feature = "metal") && matches!(target_os.as_ref(), "macos" | "ios" | "tvos" | "visionos");
    println!("cargo::rustc-check-cfg=cfg(metal_backend)");
    if metal_backend {
        println!("cargo::rustc-cfg=metal_backend");
    }

    let grammar_xgrammar = cfg!(feature = "grammar") && target_arch != "wasm32";
```

  - Target cfgs: `target_os = "macos"`, `target_family = "wasm"`, `target_vendor = "apple"`.

  Gate backend-specific code with `#[cfg(metal_backend)]` / `#[cfg(grammar_xgrammar)]`, not raw
  feature checks, so platform constraints are respected.

## 4. Error handling

- Library crates: define errors with `thiserror::Error` enums and return explicit
  `Result<T, MyError>`. Do not use `anyhow` in library code, and do not introduce a workspace-wide
  `Result<T>` alias (none exists; the only crate-local alias is in `mock-registry`).
- Binaries and `build.rs`: use `anyhow` with `.context()`, `bail!`, `anyhow!`. This applies to
  `cli`, `cli-tools`, and `backend-uzu/build/`.
- FFI-exported errors (anything crossing the bindings boundary) use the full pattern — see
  [crates/uzu/src/engine/error.rs](crates/uzu/src/engine/error.rs):

```1:16:crates/uzu/src/engine/error.rs
#[bindings::export(Error)]
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    #[error("Tokio error: {message}")]
    TokioError {
        message: String,
    },
    #[error(transparent)]
    Device(#[from] crate::device::DeviceError),
```

  Nest sub-errors with `#[error(transparent)]` + `#[from]`; give leaf variants `{ message: String }`
  payloads when they need `Clone + PartialEq`.

- Backend-generic errors are generic over `B: Backend`. Child component errors use `#[from]`;
  the backend's own error uses `#[source]` (so parents don't need `From<B::Error>`) —
  [crates/backend-uzu/src/encodable_block/attention/mod.rs](crates/backend-uzu/src/encodable_block/attention/mod.rs):

```79:88:crates/backend-uzu/src/encodable_block/attention/mod.rs
#[derive(Debug, Error)]
pub enum AttentionError<B: Backend> {
    #[error("Backend error: {0}")]
    BackendError(#[source] B::Error),
    #[error("Linear block error: {0}")]
    LinearBlockError(#[from] LinearBlockError<B>),
    #[error("QKV norm error: {0}")]
    QKVNormError(#[from] QKVNormError<B>),
    #[error("Parameter loader error: {0}")]
    ParameterLoaderError(#[from] ParameterLoaderError<B>),
}
```

- When an error must be `Clone + PartialEq` but wraps a foreign error, stringify it into a
  `Variant(String)` or `{ message: String }` and write a manual `impl From`
  ([crates/download-manager/src/download_error.rs](crates/download-manager/src/download_error.rs)).
- Propagation: prefer `?`. Use `map_err` only to wrap into a specific variant. Write a manual
  `impl From` when conversion does real work — e.g. classifying a `reqwest::Error` into
  `Timeout` vs `Network` ([crates/nagare/src/api/error.rs](crates/nagare/src/api/error.rs)).
- `unwrap` / `expect` / `panic!` / `unreachable!`: liberal in tests, benches, and build scripts. In
  production code use `?` for anything recoverable; reserve `expect`/`panic!`/`unreachable!` for
  documented invariants or genuine logic bugs (e.g. a wrong internal cache variant), and prefer
  `debug_assert!` for dev-only checks.
- `derive_more` is not used for errors — `thiserror` only.

## 5. Types, builders, constructors, serde

- Builder pattern is hand-written (not macro-generated): `with_*(&self, value) -> Self` using
  clone-and-update. See [crates/shoji/src/types/session/chat/config.rs](crates/shoji/src/types/session/chat/config.rs):

```27:35:crates/shoji/src/types/session/chat/config.rs
    pub fn with_context_length(
        &self,
        context_length: ContextLength,
    ) -> Self {
        Self {
            context_length,
            ..self.clone()
        }
    }
```

  Chain them as `Config::default().with_a(..).with_b(..)`. Take `&self` (not `self` by value);
  the rare `mut self` builders are internal GPU flag structs only.

- Constructors:
  - `create()` is the bindings-facing factory (`#[bindings::export(Method(Factory))]`) and usually
    just calls `Self::default()` or `Self::new()`.
  - Derive `Default` for plain config structs; write a manual `impl Default` for enums to return a
    sentinel variant (e.g. `SamplingPolicy::Default {}`) or when reading env vars.
  - `new()` is for internal/infra structs whose fields are all required.
  - Role/named factories exist where natural (e.g. `ChatMessage::system()`, `::user()`).

- Enum style: write unit variants as `Variant {}` (not bare `Variant`), and payload variants as
  `Variant { field: T }`. This is consistent across stream chunks, errors, and config enums, e.g.
  `ChatSessionStreamChunk::Replies { replies }`, `ContextLength::Custom { length }`.

- serde:
  - Structs frequently use `#[serde(rename_all = "snake_case")]`.
  - User-facing payload enums (`Grammar`, `ChatSpeculationPreset`) use serde's default external
    tagging, producing JSON like `{"JsonSchema":{"schema":"..."}}`.
  - Discriminated unions use internal tagging:
    `#[serde(tag = "type", rename_all = "snake_case")]`
    ([crates/shoji/src/types/model/reference.rs](crates/shoji/src/types/model/reference.rs)).
  - Telemetry-style events use adjacent tagging (`tag = "...", content = "..."`).
  - Newtypes use `#[serde(transparent)]`.

- `derive_more` (`From, Deref, AsRef, Display`) is reserved for newtype wrappers in
  `backend-uzu/build` and GPU types; do not use it in the public `shoji`/`uzu` API.

## 6. The `bindings::export` DSL (cross-language API)

Any type or method exposed to Swift / Python / TypeScript / WASM is annotated with
`#[bindings::export(...)]`; the [crates/bindings](crates/bindings) proc-macro fans out into Rust +
NAPI + PyO3 + UniFFI + WASM token streams. The available kinds (see
[crates/bindings/src/lib.rs](crates/bindings/src/lib.rs)) are: `Enumeration`, `Structure(Class)`,
`Class`, `Alias`, `Implementation`, `Method` / `Method(Factory)` / `Method(Getter)`, `Error`.

Standard shape: annotate the struct, then put exported methods in a separate
`#[bindings::export(Implementation)]` impl block
([crates/shoji/src/types/session/chat/config.rs](crates/shoji/src/types/session/chat/config.rs)):

```8:22:crates/shoji/src/types/session/chat/config.rs
#[bindings::export(Structure(Class))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ChatConfig {
    pub context_length: ContextLength,
    pub sampling_seed: SamplingSeed,
    pub speculation_preset: Option<ChatSpeculationPreset>,
}

#[bindings::export(Implementation)]
impl ChatConfig {
    #[bindings::export(Method(Factory))]
    pub fn create() -> Self {
        Self::default()
    }
}
```

Zero-arg accessors use `#[bindings::export(Method(Getter))]`. PyO3 class registration is automatic
via `inventory` (`inventory::collect!` in [crates/bindings-types/src/lib.rs](crates/bindings-types/src/lib.rs),
iterated in uzu's `#[pymodule]`). Do not hand-register classes, and do not repurpose `inventory`
for internal Rust plugin discovery.

## 7. Backend-generic & trait design (`backend-uzu`)

- The central `Backend` trait uses associated types with reciprocal bounds
  ([crates/backend-uzu/src/backends/common/backend.rs](crates/backend-uzu/src/backends/common/backend.rs)):

```5:13:crates/backend-uzu/src/backends/common/backend.rs
pub trait Backend: Debug + Clone + 'static {
    type Context: Context<Backend = Self>;
    type CommandBuffer: CommandBuffer<Backend = Self>;
    type DenseBuffer: DenseBuffer<Backend = Self>;
    type SparseBuffer: SparseBuffer<Backend = Self>;
    type Kernels: Kernels<Backend = Self>;
    type Error: Error + Debug;
```

- Parameterize all hot paths with `B: Backend` (monomorphized). Never use `dyn Backend`. Pick a
  backend at runtime via the `select_backend!` macro; iterate backends in tests via
  `for_each_backend!`.
- Command buffers encode their lifecycle as typestate associated types
  (`Initial → Encoding → Executable → Pending → Completed`).
- Encodable blocks use small `encode()` traits (`Linear<B>`, `Mlp<B>`, `KVCacheLayerTrait<B>`).
  Polymorphism is `Box<dyn Trait<B>>`, and constructors are factory methods on the trait object:
  `impl<B: Backend> dyn Linear<B> { pub fn new_*(...) -> Result<Box<dyn Linear<B>>, _> }`. Layers
  (`LayerExecutables<B>`, `Decoder<B>`) are concrete composing structs, not traits.

## 8. Proc-macro config & kernel DSLs (`backend-uzu`)

The [crates/proc-macros](crates/proc-macros) crate provides attribute macros. Prefer them over
hand-writing the equivalent boilerplate:

- `#[uzu_config]` / `#[uzu_config(super::Parent)]` for model config structs and enums. It adds
  `Debug/Clone/PartialEq/Serialize/Deserialize`, `deny_unknown_fields`, strict required-field
  deserialization, and (with a parent) a `"type"` discriminator via `monostate::MustBe!`. Example
  ([crates/backend-uzu/src/config/activation/gelu.rs](crates/backend-uzu/src/config/activation/gelu.rs)):

```1:6:crates/backend-uzu/src/config/activation/gelu.rs
use proc_macros::uzu_config;

#[uzu_config(super::Activation)]
pub struct GELU {
    pub approximate: bool,
}
```

- `#[uzu_config_abstract(VariantA, VariantB, ...)]` for config hubs: generates the `AnyFoo`
  untagged enum plus field getters ([crates/backend-uzu/src/config/activation/mod.rs](crates/backend-uzu/src/config/activation/mod.rs)).
- `#[kernel(Name)]` with `#[variants(...)]` / `#[specialize]` / `#[optional(...)]` for CPU kernels;
  the macro includes generated code from `OUT_DIR`.
- Use `#[uzu_test]` instead of `#[test]`, and `#[uzu_bench]` instead of `#[criterion]`. These
  enable the Apple device-runner aliasing configured in the workspace metadata.

## 9. Async & streaming model

- The inference path in `backend-uzu` is synchronous. Streaming bridges sync GPU work to async
  consumers with `std::thread::spawn` + `futures` mpsc channels, plus
  `tokio_util::sync::CancellationToken` for cancellation.
- API/session boundaries in `shoji` and `nagare` expose
  `Pin<Box<dyn Future<...> + Send>>` / `Pin<Box<dyn Stream<...> + Send>>` rather than
  `async fn in trait`.
- `async-trait` is used only in `download-manager` and build scripts. `tokio` powers the `nagare`,
  CLI, and server runtimes.
- Naming caveat: `prepare_async` / `async_generate` on the language-model generator refer to GPU
  command-buffer pipelining, not Rust `async`/`await`.

## 10. Testing

- `backend-uzu` uses three tiers plus benches:
  - `unit/` files wired into source with `#[cfg(test)] #[path = "../../unit/.../foo_test.rs"] mod tests;`.
  - `tests/integration` and `tests/performance` binaries.
  - `benches/` (Criterion, declared as `[[bench]] name = "main"`).
  Shared helpers live in `tests/common/` and are `#[macro_use]`-included from `lib.rs` under `#[cfg(test)]`.
- Prefer separate `*_test.rs` files over inline `#[cfg(test)] mod tests { ... }`; use inline only
  for small pure-logic cases.
- Always put the test module at the **end of the file**, after all production code. This holds for
  both the `#[cfg(test)] #[path = "..."] mod tests;` wiring and inline `#[cfg(test)] mod tests { ... }`
  blocks — never insert tests in the middle of a file.
- Use `#[uzu_test]` and wrap backend-dependent bodies in `for_each_backend!`. Gate Metal-only tests
  with `#![cfg(metal_backend)]` (or `#![cfg(all(metal_backend, grammar_xgrammar))]`). Mark
  model-loading tests `#[tag(heavy)]` (`test-tag`) and env-dependent ones `#[ignore]`.
- Helpers in use: `rstest` for parameterized cases, `proptest` for CPU/Metal parity,
  `is_close` plus a custom `assert_eq_float` for numeric tolerance, and `wiremock`/`tempfile` in
  `download-manager`. Load models via `get_test_model_path()` from `workspace/models/{version}/`
  (`TEST_MODEL` overrides the dir) after running `./scripts/download_test_model.sh`.
- Naming: test functions `test_*`; kernel dtype matrices `test_{in}_{scale}_{out}_{accum}`;
  benches `bench_*`. Test files use the `*_test.rs` suffix.

## 11. Quick do / don't checklist

- Run `cargo fmt`; keep three import groups and vertical fn params.
- Add new dependencies to `[workspace.dependencies]`, reference with `dep.workspace = true`.
- Add `[lints] workspace = true` to every new crate.
- Libraries: `thiserror` + explicit `Result<T, E>`. Binaries / `build.rs`: `anyhow`.
- Backend-generic errors: `#[from]` for child errors, `#[source]` for `B::Error`.
- Public API: `#[bindings::export(...)]` with a `create()` factory and `with_*(&self) -> Self` builders.
- Parameterize over `B: Backend` (monomorphized); never `dyn Backend`.
- Use `#[uzu_config]` / `#[uzu_config_abstract]` for model config and `#[kernel]` for CPU kernels.
- Use `#[uzu_test]` / `#[uzu_bench]`, `for_each_backend!`, and `metal_backend` / `grammar_xgrammar` cfg gates.
- Put public types in `shoji`, session logic in `nagare`, orchestration in `uzu`.
