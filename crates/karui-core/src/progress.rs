//! Reading `ffmpeg -progress pipe:1`.
//!
//! ffmpeg writes blocks of `key=value` lines, each closed by
//! `progress=continue` or, once, `progress=end`. Only the closing line means
//! a block is complete, so a snapshot is emitted there and nowhere else.

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// How far into the source the encoder has reached.
    pub out_time_secs: f64,
    /// Multiples of real time, e.g. `2.5` for `2.5x`.
    pub speed: Option<f64>,
    /// Bytes written so far.
    pub total_size: Option<u64>,
    /// `true` on the last block.
    pub done: bool,
}

impl Snapshot {
    /// Fraction complete, when the duration is known.
    pub fn fraction(&self, duration_secs: Option<f64>) -> Option<f64> {
        let duration = duration_secs.filter(|d| *d > 0.0)?;
        if self.done {
            return Some(1.0);
        }
        Some((self.out_time_secs / duration).clamp(0.0, 1.0))
    }

    /// Seconds left at the current speed.
    pub fn eta_secs(&self, duration_secs: Option<f64>) -> Option<f64> {
        let duration = duration_secs?;
        let speed = self.speed.filter(|s| *s > 0.0)?;
        Some(((duration - self.out_time_secs) / speed).max(0.0))
    }
}

#[derive(Default)]
pub struct Parser {
    current: Snapshot,
}

impl Parser {
    pub fn feed(&mut self, line: &str) -> Option<Snapshot> {
        let (key, value) = line.trim().split_once('=')?;
        let value = value.trim();
        match key {
            // `out_time_ms` is also microseconds, despite its name — an
            // ffmpeg quirk kept for compatibility. `out_time_us` says so.
            "out_time_us" | "out_time_ms" => {
                if let Ok(us) = value.parse::<i64>() {
                    self.current.out_time_secs = us.max(0) as f64 / 1_000_000.0;
                }
            }
            "speed" => {
                self.current.speed = value.trim_end_matches('x').trim().parse().ok();
            }
            "total_size" => {
                self.current.total_size = value.parse().ok();
            }
            "progress" => {
                self.current.done = value == "end";
                return Some(self.current);
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_one_snapshot_per_block() {
        let mut parser = Parser::default();
        let block = "frame=120\nfps=60.0\ntotal_size=524288\nout_time_us=4000000\n\
                     out_time_ms=4000000\nout_time=00:00:04.000000\nspeed=2.01x\nprogress=continue\n";
        let snapshots: Vec<_> = block.lines().filter_map(|l| parser.feed(l)).collect();
        assert_eq!(snapshots.len(), 1);
        let s = snapshots[0];
        assert_eq!(s.out_time_secs, 4.0);
        assert_eq!(s.speed, Some(2.01));
        assert_eq!(s.total_size, Some(524_288));
        assert_eq!(s.fraction(Some(8.0)), Some(0.5));
        assert!((s.eta_secs(Some(8.0)).expect("eta") - 1.99).abs() < 0.01);
    }

    #[test]
    fn tolerates_not_yet_known_values() {
        let mut parser = Parser::default();
        for line in ["out_time_us=N/A", "speed=N/A", "progress=continue"] {
            if let Some(s) = parser.feed(line) {
                assert_eq!(s.out_time_secs, 0.0);
                assert_eq!(s.speed, None);
                assert_eq!(s.fraction(None), None);
            }
        }
    }

    #[test]
    fn end_reads_complete() {
        let mut parser = Parser::default();
        parser.feed("out_time_us=9900000");
        let s = parser.feed("progress=end").expect("snapshot");
        assert!(s.done);
        assert_eq!(s.fraction(Some(10.0)), Some(1.0));
    }
}
