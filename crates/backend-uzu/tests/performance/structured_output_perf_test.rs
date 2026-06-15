#![cfg(all(metal_backend, grammar_xgrammar))]

use std::path::Path;

use backend_uzu::{
    backends::metal::Metal,
    session::{
        ChatSession,
        types::{Error, FinishReason, Output},
    },
};
use proc_macros::uzu_test;

use crate::common::structured_output_fixtures::{
    Scenario, ScenarioKind, benchmark_input, extract_response_text, model_path_for_no_thinking,
    model_path_for_thinking, no_thinking_limits, no_thinking_scenarios, parse_calendar_event, parse_json_object,
    with_thinking_limits, with_thinking_scenarios,
};

const WARMUP_ITERS: usize = 1;
const MEASURE_ITERS: usize = 5;

#[derive(Debug, Clone)]
struct SampleStats {
    total_ms: f64,
    prefill_ms: f64,
    generate_ms: f64,
}

#[derive(Debug, Clone)]
struct ScenarioSummary {
    scenario_name: String,
    tokens_limit: u64,
    median_total_ms: f64,
    median_prefill_ms: f64,
    median_generate_ms: f64,
}

#[derive(Debug, Clone)]
struct CompileProxySummary {
    tokens_limit: u64,
    structured_cold_ms: f64,
    structured_warm_median_ms: f64,
    plain_forced_sync_cold_ms: f64,
    plain_forced_sync_warm_median_ms: f64,
}

fn scenario_runs(
    model_path: &Path,
    scenario: Scenario,
    tokens_limit: u64,
) -> Result<Vec<SampleStats>, Error> {
    let mut session = ChatSession::new_with_backend::<Metal>(model_path.to_path_buf(), scenario.decoding_config())?;
    let input = benchmark_input();
    let mut samples = Vec::with_capacity(MEASURE_ITERS);

    for iter_idx in 0..(WARMUP_ITERS + MEASURE_ITERS) {
        session.reset()?;
        let output = session.run_internal(
            input.clone(),
            scenario.with_tokens_limit(tokens_limit),
            None::<fn(Output) -> bool>,
        )?;
        ensure_scenario_output_valid(scenario, &output);
        if iter_idx >= WARMUP_ITERS {
            let total_ms = output.stats.total_stats.duration * 1000.0;
            let prefill_ms = output.stats.prefill_stats.duration * 1000.0;
            let generate_ms = output.stats.generate_stats.as_ref().map(|stats| stats.duration * 1000.0).unwrap_or(0.0);
            samples.push(SampleStats {
                total_ms,
                prefill_ms,
                generate_ms,
            });
        }
    }

    Ok(samples)
}

fn ensure_scenario_output_valid(
    scenario: Scenario,
    output: &Output,
) {
    let is_length_terminated = matches!(output.finish_reason, Some(FinishReason::Length));
    match scenario.kind {
        ScenarioKind::StructuredCalendarSchema => {
            if is_length_terminated {
                return;
            }
            parse_calendar_event(output).unwrap_or_else(|error| {
                panic!(
                    "Scenario '{}' produced invalid CalendarEvent JSON: {error}. Response: {}",
                    scenario.name,
                    extract_response_text(output),
                )
            });
        },
        ScenarioKind::StructuredBuiltinJson => {
            if is_length_terminated {
                return;
            }
            let json_value = parse_json_object(output).unwrap_or_else(|error| {
                panic!(
                    "Scenario '{}' produced invalid JSON object: {error}. Response: {}",
                    scenario.name,
                    extract_response_text(output),
                )
            });
            assert!(
                json_value.is_object(),
                "Scenario '{}' must return a JSON object. Response: {}",
                scenario.name,
                extract_response_text(output),
            );
        },
        ScenarioKind::PlainAsyncCandidate | ScenarioKind::PlainForcedSync => {},
    }
}

fn summarize(
    scenario_name: &str,
    tokens_limit: u64,
    samples: &[SampleStats],
) -> ScenarioSummary {
    let median_total_ms = median(samples.iter().map(|item| item.total_ms).collect());
    let median_prefill_ms = median(samples.iter().map(|item| item.prefill_ms).collect());
    let median_generate_ms = median(samples.iter().map(|item| item.generate_ms).collect());
    ScenarioSummary {
        scenario_name: scenario_name.to_string(),
        tokens_limit,
        median_total_ms,
        median_prefill_ms,
        median_generate_ms,
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).expect("metric should be sortable"));
    values[values.len() / 2]
}

fn ratio(
    numerator: f64,
    denominator: f64,
) -> f64 {
    if denominator <= f64::EPSILON {
        f64::NAN
    } else {
        numerator / denominator
    }
}

fn classify_ratio_delta(ratio_value: f64) -> &'static str {
    if ratio_value.is_nan() {
        "n/a"
    } else if ratio_value <= 1.05 {
        "<=5% (noise/neutral)"
    } else if ratio_value <= 1.10 {
        "5-10% (noticeable, rerun)"
    } else {
        ">10% (material change)"
    }
}

fn cold_warm_summary(
    model_path: &Path,
    scenario: Scenario,
    tokens_limit: u64,
) -> Result<(f64, f64), Error> {
    let mut session = ChatSession::new_with_backend::<Metal>(model_path.to_path_buf(), scenario.decoding_config())?;
    let input = benchmark_input();

    session.reset()?;
    let cold_output =
        session.run_internal(input.clone(), scenario.with_tokens_limit(tokens_limit), None::<fn(Output) -> bool>)?;
    ensure_scenario_output_valid(scenario, &cold_output);
    let cold_ms = cold_output.stats.total_stats.duration * 1000.0;

    let mut warm_samples = Vec::with_capacity(MEASURE_ITERS);
    for iter_idx in 0..(WARMUP_ITERS + MEASURE_ITERS) {
        session.reset()?;
        let output = session.run_internal(
            input.clone(),
            scenario.with_tokens_limit(tokens_limit),
            None::<fn(Output) -> bool>,
        )?;
        ensure_scenario_output_valid(scenario, &output);
        if iter_idx >= WARMUP_ITERS {
            warm_samples.push(output.stats.total_stats.duration * 1000.0);
        }
    }

    Ok((cold_ms, median(warm_samples)))
}

fn print_report(
    mode_label: &str,
    summaries: &[ScenarioSummary],
    compile_proxy_summary: &CompileProxySummary,
) {
    eprintln!("\n=== Structured Output performance ({mode_label}) ===");
    for summary in summaries {
        eprintln!(
            "  {:<40} tokens={:<4} total={:8.3}ms prefill={:8.3}ms generate={:8.3}ms",
            summary.scenario_name,
            summary.tokens_limit,
            summary.median_total_ms,
            summary.median_prefill_ms,
            summary.median_generate_ms,
        );
    }

    let structured_one_off = compile_proxy_summary.structured_cold_ms - compile_proxy_summary.structured_warm_median_ms;
    let plain_one_off =
        compile_proxy_summary.plain_forced_sync_cold_ms - compile_proxy_summary.plain_forced_sync_warm_median_ms;
    let compile_proxy_ms = structured_one_off - plain_one_off;
    eprintln!(
        "  compile_proxy tokens={} structured(cold={:8.3}ms warm={:8.3}ms) \
         plain_forced_sync(cold={:8.3}ms warm={:8.3}ms) approx_compile={:8.3}ms",
        compile_proxy_summary.tokens_limit,
        compile_proxy_summary.structured_cold_ms,
        compile_proxy_summary.structured_warm_median_ms,
        compile_proxy_summary.plain_forced_sync_cold_ms,
        compile_proxy_summary.plain_forced_sync_warm_median_ms,
        compile_proxy_ms,
    );

    for &tokens_limit in &[32_u64, 128, 256, 512, 1024] {
        let plain_async = summaries.iter().find(|summary| {
            summary.tokens_limit == tokens_limit
                && summary.scenario_name.starts_with("plain_")
                && !summary.scenario_name.contains("forced_sync")
        });
        let plain_forced_sync = summaries.iter().find(|summary| {
            summary.tokens_limit == tokens_limit && summary.scenario_name.contains("plain_forced_sync")
        });
        let structured_schema = summaries.iter().find(|summary| {
            summary.tokens_limit == tokens_limit && summary.scenario_name.contains("structured_calendar_event")
        });
        let structured_builtin = summaries.iter().find(|summary| {
            summary.tokens_limit == tokens_limit && summary.scenario_name.contains("structured_builtin_json")
        });

        if let (Some(plain_async), Some(plain_forced_sync), Some(structured_schema), Some(structured_builtin)) =
            (plain_async, plain_forced_sync, structured_schema, structured_builtin)
        {
            let ratio_struct_plain_async = ratio(structured_schema.median_total_ms, plain_async.median_total_ms);
            let ratio_struct_plain_sync = ratio(structured_schema.median_total_ms, plain_forced_sync.median_total_ms);
            let ratio_builtin_plain_sync = ratio(structured_builtin.median_total_ms, plain_forced_sync.median_total_ms);
            eprintln!(
                "  ratios tokens={tokens_limit:<4} struct/plain_async={ratio_struct_plain_async:>6.3} ({}) \
                 struct/plain_forced_sync={ratio_struct_plain_sync:>6.3} ({}) \
                 builtin/plain_forced_sync={ratio_builtin_plain_sync:>6.3} ({})",
                classify_ratio_delta(ratio_struct_plain_async),
                classify_ratio_delta(ratio_struct_plain_sync),
                classify_ratio_delta(ratio_builtin_plain_sync),
            );
        }
    }
}

fn run_suite(
    mode_label: &str,
    model_path: &Path,
    scenarios: &[Scenario],
    tokens_limits: &[u64],
    enable_thinking: bool,
) {
    let mut summaries: Vec<ScenarioSummary> = Vec::new();
    for scenario in scenarios {
        for &tokens_limit in tokens_limits {
            match scenario_runs(model_path, *scenario, tokens_limit) {
                Ok(samples) => summaries.push(summarize(scenario.name, tokens_limit, &samples)),
                Err(Error::UnsupportedSpeculatorConfigForModel) if scenario.is_plain_forced_sync() => {
                    eprintln!(
                        "Skipping scenario '{}' for {mode_label}: forced-sync speculator unsupported by model.",
                        scenario.name
                    );
                },
                Err(error) => {
                    panic!(
                        "Scenario '{}' failed for {mode_label} (tokens_limit={tokens_limit}): {error}",
                        scenario.name,
                    );
                },
            }
        }
    }

    let compile_proxy_tokens_limit = if enable_thinking {
        512
    } else {
        32
    };
    let plain_forced_sync = scenarios
        .iter()
        .find(|scenario| scenario.kind == ScenarioKind::PlainForcedSync)
        .copied()
        .expect("plain forced-sync scenario must exist");
    let structured_schema = scenarios
        .iter()
        .find(|scenario| scenario.kind == ScenarioKind::StructuredCalendarSchema)
        .copied()
        .expect("structured schema scenario must exist");

    let (structured_cold_ms, structured_warm_median_ms) =
        cold_warm_summary(model_path, structured_schema, compile_proxy_tokens_limit)
            .expect("structured cold/warm summary should succeed");
    let (plain_forced_sync_cold_ms, plain_forced_sync_warm_median_ms) =
        cold_warm_summary(model_path, plain_forced_sync, compile_proxy_tokens_limit)
            .expect("plain forced-sync cold/warm summary should succeed");
    let compile_proxy_summary = CompileProxySummary {
        tokens_limit: compile_proxy_tokens_limit,
        structured_cold_ms,
        structured_warm_median_ms,
        plain_forced_sync_cold_ms,
        plain_forced_sync_warm_median_ms,
    };
    print_report(mode_label, &summaries, &compile_proxy_summary);
}

#[uzu_test]
#[ignore = "heavy performance benchmark; run explicitly with --ignored structured_output"]
fn structured_output_perf_no_thinking() {
    let model_path = model_path_for_no_thinking();
    let scenarios = no_thinking_scenarios();
    let limits = no_thinking_limits();
    run_suite("no_thinking", model_path.as_path(), &scenarios, limits.as_slice(), false);
}

#[uzu_test]
#[ignore = "requires THINKING_TEST_MODEL; run explicitly with --ignored structured_output_thinking"]
fn structured_output_perf_thinking() {
    let Some(model_path) = model_path_for_thinking() else {
        eprintln!("Skipping thinking suite: set THINKING_TEST_MODEL to enable thinking scenarios.");
        return;
    };

    let scenarios = with_thinking_scenarios();
    let limits = with_thinking_limits();
    run_suite("thinking", model_path.as_path(), &scenarios, limits.as_slice(), true);
}
