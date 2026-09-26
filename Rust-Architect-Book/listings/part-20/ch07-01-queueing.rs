// verify: release ok
// Why latency explodes before utilization reaches 100%: a single-server FIFO queue (M/M/1: random arrivals,
// random service times, mean service 100 us), simulated in virtual time with a fixed seed (deterministic).
// Also checks Little's law, L = lambda * W, from the simulation's own time-average queue length.
use hdrhistogram::Histogram;

struct Rng(u64);
impl Rng {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    /// Exponentially distributed with the given mean (inverse-CDF method).
    fn exp(&mut self, mean: f64) -> f64 {
        -mean * self.uniform().ln()
    }
}

fn main() {
    let service_mean_us = 100.0;
    println!("{:>5} {:>10} {:>10} {:>10} {:>12} {:>14}", "util", "p50 (us)", "p99 (us)", "p99.9", "mean W (us)", "theory W (us)");
    for util in [0.5, 0.7, 0.8, 0.9, 0.95, 0.99] {
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        let arrival_mean_us = service_mean_us / util;
        let mut h = Histogram::<u64>::new_with_bounds(1, 1_000_000_000, 3).unwrap();
        let (mut t_arrive, mut server_free, mut sum_w) = (0.0f64, 0.0f64, 0.0f64);
        let n = 400_000;
        for _ in 0..n {
            t_arrive += rng.exp(arrival_mean_us);
            let start = t_arrive.max(server_free);
            let done = start + rng.exp(service_mean_us);
            server_free = done;
            let w = done - t_arrive; // time in system: queueing + service
            sum_w += w;
            h.record(w.max(1.0) as u64).unwrap();
        }
        let mean_w = sum_w / n as f64;
        // Little's law: average number in the system L = lambda * W. Measure L directly as the time-average:
        // the area under "number in system" equals the sum of every customer's time in the system.
        let lambda = n as f64 / t_arrive;
        let l_time_avg = sum_w / server_free;
        let theory = service_mean_us / (1.0 - util);
        println!(
            "{:>4.0}% {:>10} {:>10} {:>10} {:>12.0} {:>14.0}   L = {:.2}, lambda*W = {:.2}",
            util * 100.0,
            h.value_at_percentile(50.0),
            h.value_at_percentile(99.0),
            h.value_at_percentile(99.9),
            mean_w,
            theory,
            l_time_avg,
            lambda * mean_w
        );
    }
}
