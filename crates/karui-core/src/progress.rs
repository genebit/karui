//! Reading `ffmpeg -progress pipe:1`.
//!
//! ffmpeg writes blocks of `key=value` lines, each closed by
//! `progress=continue` or, once, `progress=end`. Only the closing line means
//! a block is complete, so a snapshot is emitted there and nowhere else.

use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// How far into the source the encoder has reached.
    pub out_time_secs: f64,
    /// Multiples of real time, e.g. `2.5` for `2.5x`.
    pub speed: Option<f64>,
    /// Bytes written so far.
    pub total_size: Option<u64>,
    /// Frames written so far.
    pub frame: Option<u64>,
    /// ffmpeg's frames a second, averaged over the whole encode so far.
    pub average_fps: Option<f64>,
    /// Output bitrate so far, in kilobits a second.
    pub bitrate_kbps: Option<f64>,
    /// The video quantizer of the latest frame: what CRF works out to there.
    pub quantizer: Option<f64>,
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

    /// The output's final size, from what has been written for the fraction
    /// done. Withheld for the first 2%, where the file header and the
    /// encoder's start-up dominate and would project far too much.
    pub fn projected_bytes(&self, duration_secs: Option<f64>) -> Option<u64> {
        let fraction = self.fraction(duration_secs).filter(|f| *f >= 0.02)?;
        let written = self.total_size.filter(|b| *b > 0)?;
        Some((written as f64 / fraction).round() as u64)
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
            "frame" => self.current.frame = value.parse().ok(),
            "fps" => {
                self.current.average_fps = value.parse().ok().filter(|f: &f64| *f > 0.0);
            }
            // `1234.5kbits/s`, or `N/A` before the first packet.
            "bitrate" => {
                self.current.bitrate_kbps = value.trim_end_matches("kbits/s").trim().parse().ok();
            }
            // The first output stream, which is always the video.
            "stream_0_0_q" => {
                self.current.quantizer = value.parse().ok().filter(|q: &f64| *q >= 0.0);
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

/// How smoothly [`Meter`] follows the instantaneous rate: each report moves
/// it this far toward the newest reading, so a one-off stall at a scene cut
/// shows without the line jumping about every half second.
const SMOOTHING: f64 = 0.4;

/// Frames a second right now, from the frames done between reports.
///
/// ffmpeg's own `fps` is the average since the start, which hides a slow
/// stretch an hour into a long encode.
#[derive(Clone, Copy, Debug, Default)]
pub struct Meter {
    last: Option<(Instant, u64)>,
    rate: Option<f64>,
}

impl Meter {
    pub fn update(&mut self, at: Instant, frame: u64) -> Option<f64> {
        if let Some((then, before)) = self.last {
            let secs = at.duration_since(then).as_secs_f64();
            if secs > 0.0 && frame >= before {
                let now = (frame - before) as f64 / secs;
                self.rate = Some(match self.rate {
                    Some(rate) => rate + SMOOTHING * (now - rate),
                    None => now,
                });
            }
        }
        self.last = Some((at, frame));
        self.rate
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
    fn reads_the_encoder_details() {
        let mut parser = Parser::default();
        let block = "frame=3456\nfps=8.12\nstream_0_0_q=31.5\nbitrate=21410.3kbits/s\n\
                     total_size=412300000\nout_time_us=144000000\nspeed=0.34x\nprogress=continue\n";
        let s = block
            .lines()
            .find_map(|l| parser.feed(l))
            .expect("snapshot");
        assert_eq!(s.frame, Some(3456));
        assert_eq!(s.average_fps, Some(8.12));
        assert_eq!(s.quantizer, Some(31.5));
        assert_eq!(s.bitrate_kbps, Some(21410.3));

        let mut early = Parser::default();
        let s = [
            "bitrate=N/A",
            "stream_0_0_q=-1.0",
            "fps=0.00",
            "progress=continue",
        ]
        .into_iter()
        .find_map(|l| early.feed(l))
        .expect("snapshot");
        assert_eq!(
            (s.bitrate_kbps, s.quantizer, s.average_fps),
            (None, None, None)
        );
    }

    #[test]
    fn projects_the_final_size_once_past_the_start() {
        let s = Snapshot {
            out_time_secs: 25.0,
            total_size: Some(5_000_000),
            ..Default::default()
        };
        assert_eq!(s.projected_bytes(Some(100.0)), Some(20_000_000));
        let early = Snapshot {
            out_time_secs: 1.0,
            ..s
        };
        assert_eq!(early.projected_bytes(Some(100.0)), None);
        assert_eq!(s.projected_bytes(None), None);
    }

    #[test]
    fn meter_tracks_the_current_rate_smoothly() {
        let start = Instant::now();
        let at = |ms| start + std::time::Duration::from_millis(ms);
        let mut meter = Meter::default();
        assert_eq!(meter.update(at(0), 0), None);
        // 5 frames in half a second: 10 fps.
        assert_eq!(meter.update(at(500), 5), Some(10.0));
        // A stall moves it partway, not to zero.
        let stalled = meter.update(at(1000), 5).expect("rate");
        assert!((stalled - 6.0).abs() < 1e-9);
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
