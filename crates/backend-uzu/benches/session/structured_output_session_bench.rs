use std::{path::Path, time::Duration};

use backend_uzu::{
    backends::common::Backend,
    session::{
        ChatSession,
        types::{Error, Input, Output},
    },
};
use criterion::{BenchmarkId, Criterion};
use proc_macros::uzu_bench;

use crate::common::{
    metrics::wait_gpu_cooldown,
    structured_output_fixtures::{
        Scenario, benchmark_input, model_path_for_no_thinking, model_path_for_thinking, no_thinking_limits,
        no_thinking_scenarios, with_thinking_limits, with_thinking_scenarios,
    },
};

fn create_session<B: Backend>(
    model_path: &Path,
    scenario: &Scenario,
) -> Result<ChatSession, Error> {
    ChatSession::new_with_backend::<B>(model_path.to_path_buf(), scenario.decoding_config())
}

fn run_session(
    session: &mut ChatSession,
    input: Input,
    scenario: &Scenario,
    tokens_limit: u64,
) -> Duration {
    let result = session
        .run_internal(input, scenario.with_tokens_limit(tokens_limit), None::<fn(Output) -> bool>)
        .expect("benchmark run should succeed");
    Duration::from_secs_f64(result.stats.total_stats.duration)
}

fn bench_group_for_backend<B: Backend>(
    c: &mut Criterion,
    group_name: &str,
    model_path: &Path,
    scenarios: &[Scenario],
    tokens_limits: &[u64],
) {
    wait_gpu_cooldown();

    let input = benchmark_input();
    let mut group = c.benchmark_group(group_name);
    group.sample_size(10);

    for scenario in scenarios {
        for &tokens_limit in tokens_limits {
            let mut session = match create_session::<B>(model_path, scenario) {
                Ok(session) => session,
                Err(Error::UnsupportedSpeculatorConfigForModel) if scenario.is_plain_forced_sync() => {
                    eprintln!(
                        "Skipping scenario '{}' for backend {}: forced-sync speculator unsupported by model.",
                        scenario.name,
                        crate::common::type_short_name::<B>(),
                    );
                    continue;
                },
                Err(error) => {
                    panic!(
                        "Failed to create session for scenario '{}' on backend {}: {error}",
                        scenario.name,
                        crate::common::type_short_name::<B>(),
                    );
                },
            };

            let bench_id = BenchmarkId::new(scenario.name, format!("tokens_{tokens_limit}"));
            group.bench_function(bench_id, |b| {
                b.iter_custom(|n_iter| {
                    let mut total_duration = Duration::from_secs(0);
                    for _ in 0..n_iter {
                        session.reset().expect("session reset should succeed");
                        total_duration += run_session(&mut session, input.clone(), scenario, tokens_limit);
                    }
                    total_duration
                });
            });
        }
    }

    group.finish();
}

#[uzu_bench]
fn bench_chat_session_structured_output(c: &mut Criterion) {
    let no_thinking_scenarios = no_thinking_scenarios();
    let no_thinking_limits = no_thinking_limits();

    for_each_non_cpu_backend!(|B| {
        let no_thinking_model = model_path_for_no_thinking();
        bench_group_for_backend::<B>(
            c,
            "ChatSession structured/no_thinking",
            no_thinking_model.as_path(),
            &no_thinking_scenarios,
            &no_thinking_limits,
        );

        let thinking_scenarios = with_thinking_scenarios();
        let thinking_limits = with_thinking_limits();
        if let Some(thinking_model) = model_path_for_thinking() {
            bench_group_for_backend::<B>(
                c,
                "ChatSession structured/thinking",
                thinking_model.as_path(),
                &thinking_scenarios,
                &thinking_limits,
            );
        } else {
            eprintln!(
                "Skipping ChatSession structured/thinking: set THINKING_TEST_MODEL to enable thinking scenarios."
            );
        }
    });
}
