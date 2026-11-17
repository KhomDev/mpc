//! OT-based ECDSA triple generation by batch size: what one participant computes, sends and
//! waits for per batch. Like the node, it generates triples among exactly `t` participants, so
//! `NUM_PARTICIPANTS` is `t` and `THRESHOLD` is ignored. `TRIPLE_BATCH_SIZES` takes a comma
//! separated list of batch sizes.
#![allow(clippy::indexing_slicing, clippy::missing_panics_doc)]

use std::env;
use std::time::Duration;

use average::{Estimate, Quantile};
use rand::RngCore;
use rand_core::SeedableRng;

use threshold_signatures::{
    ReconstructionThreshold,
    ecdsa::ot_based_ecdsa::triples::generate_triple_many,
    participants::Participant,
    test_utils::{
        BenchConfig, LatencyModel, MockCryptoRng, SimulationMetrics,
        generate_participants_with_random_ids, run_simulation,
    },
};

const DEFAULT_BATCH_SIZES: [usize; 6] = [2, 4, 8, 16, 32, 64];

fn main() {
    let config = BenchConfig::from_env();
    let batch_sizes = batch_sizes_from_env();

    println!("Protocol simulation: OT-based ECDSA triple generation by batch size");
    println!(
        "Participants (= threshold): {}, latency: {}ms, samples: {}",
        config.num_participants,
        config.latency_ms(),
        config.samples
    );
    println!();

    let mut setup_rng = MockCryptoRng::seed_from_u64(42);
    let participants =
        generate_participants_with_random_ids(config.num_participants, &mut setup_rng);

    config.warmup(&|| {
        run_batch(&participants, 2, &config.latency);
    });

    println!(
        "| Batch | Virtual time | Compute/participant | Compute/triple | Max burst \
         | Sent/participant | Sent/triple | Messages sent/participant | Max message |"
    );
    println!("|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for batch_size in batch_sizes {
        let samples: Vec<SimulationMetrics> = (0..config.samples)
            .map(|i| {
                eprint!("\rbatch {batch_size}: sample {}/{}", i + 1, config.samples);
                run_batch(&participants, batch_size, &config.latency)
            })
            .collect();
        eprintln!();
        let first = &samples[0];
        for sample in &samples {
            sample.assert_deterministic(first, &format!("batch {batch_size}"));
        }

        let triples = u32::try_from(batch_size).expect("batch size fits in u32");
        let participants_num =
            u64::try_from(config.num_participants).expect("participant count fits in u64");
        let virtual_time = median(samples.iter().map(SimulationMetrics::virtual_time_ms));
        let compute = median(samples.iter().map(|m| millis(m.avg_compute_time())));
        let max_burst = median(samples.iter().map(|m| millis(m.max_burst_time)));
        let sent = first.total_bytes_sent / participants_num;
        let messages = first.total_messages_sent / participants_num;
        println!(
            "| {batch_size} | {virtual_time:.0} ms | {compute:.1} ms | {:.2} ms | {max_burst:.1} ms \
             | {} KB | {} KB | {messages} | {} KB |",
            compute / f64::from(triples),
            sent / 1000,
            sent / u64::from(triples) / 1000,
            first.max_message_bytes / 1000,
        );
    }
}

fn run_batch(
    participants: &[Participant],
    batch_size: usize,
    latency: &LatencyModel,
) -> SimulationMetrics {
    let threshold = ReconstructionThreshold::from(participants.len());
    let mut rng = MockCryptoRng::seed_from_u64(55);
    let protocols: Vec<_> = participants
        .iter()
        .map(|&p| {
            let rng_p = MockCryptoRng::seed_from_u64(rng.next_u64());
            let protocol = generate_triple_many(participants, p, threshold, batch_size, rng_p)
                .expect("Triple generation should succeed");
            (p, protocol)
        })
        .collect();
    let (results, metrics) = run_simulation(protocols, latency);
    assert_eq!(results.len(), participants.len());
    assert!(
        results
            .iter()
            .all(|(_, triples)| triples.len() == batch_size)
    );
    metrics
}

fn batch_sizes_from_env() -> Vec<usize> {
    env::var("TRIPLE_BATCH_SIZES").map_or_else(
        |_| DEFAULT_BATCH_SIZES.to_vec(),
        |sizes| {
            sizes
                .split(',')
                .map(|size| {
                    size.trim()
                        .parse()
                        .expect("TRIPLE_BATCH_SIZES is a comma separated list of batch sizes")
                })
                .collect()
        },
    )
}

fn median(values: impl Iterator<Item = f64>) -> f64 {
    let mut median = Quantile::new(0.5);
    values.for_each(|value| median.add(value));
    median.quantile()
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}
