//! Test-only measurement helpers for the Read Aloud optimisation pass (Task 8):
//! process RAM (working set / private bytes), process and thread CPU time, and
//! p50 / p95 summaries. Used by the `#[ignore]` benches; run them in release:
//! `cargo test --release <name> -- --ignored --nocapture --test-threads=1`.

use std::time::Duration;

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread, GetProcessTimes, GetThreadTimes};

/// (working set MB, private bytes MB, peak working set MB) of this process.
pub fn mem() -> (f64, f64, f64) {
    let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
    let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    c.cb = cb;
    let ok = unsafe {
        GetProcessMemoryInfo(GetCurrentProcess(), &mut c as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS, cb)
    };
    if ok.is_err() {
        return (0.0, 0.0, 0.0);
    }
    let mb = |b: usize| b as f64 / 1_048_576.0;
    (mb(c.WorkingSetSize), mb(c.PrivateUsage), mb(c.PeakWorkingSetSize))
}

fn ft(f: FILETIME) -> Duration {
    Duration::from_nanos((((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64) * 100)
}

/// User + kernel CPU time of the whole process so far.
pub fn process_cpu() -> Duration {
    let (mut a, mut b, mut k, mut u) = Default::default();
    if unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut k, &mut u) }.is_err() {
        return Duration::ZERO;
    }
    ft(k) + ft(u)
}

/// User + kernel CPU time of the calling thread so far.
pub fn thread_cpu() -> Duration {
    let (mut a, mut b, mut k, mut u) = Default::default();
    if unsafe { GetThreadTimes(GetCurrentThread(), &mut a, &mut b, &mut k, &mut u) }.is_err() {
        return Duration::ZERO;
    }
    ft(k) + ft(u)
}

/// Logical processors.
pub fn cores() -> usize {
    num_cpus::get()
}

/// p50 / p95 / max of millisecond samples ("-" when empty).
pub fn pct(ms: &[f64]) -> String {
    if ms.is_empty() {
        return "-".into();
    }
    let mut v = ms.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    format!("p50 {:.2} / p95 {:.2} / max {:.2} ms (n={})", at(0.5), at(0.95), at(1.0), v.len())
}

pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_answer_without_panicking() {
        let (ws, private, peak) = mem();
        assert!(ws > 0.0 && private > 0.0 && peak >= ws);
        let t = thread_cpu();
        let mut x = 0u64;
        for i in 0..2_000_000u64 {
            x = x.wrapping_add(i * i);
        }
        std::hint::black_box(x);
        assert!(thread_cpu() >= t);
        assert!(process_cpu() >= thread_cpu());
        assert_eq!(pct(&[]), "-");
        assert!(pct(&[1.0, 2.0, 3.0]).starts_with("p50 2.00"));
    }
}
