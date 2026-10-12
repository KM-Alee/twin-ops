use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSignal {
    ConnectionRefused,
    Timeout,
    PermissionDenied,
    OutOfMemory,
    ErrorMarker,
    WarningMarker,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogSignalCount {
    pub signal: LogSignal,
    pub matches: usize,
}

pub(crate) struct LogClassifier {
    window: [u8; 18],
    used: usize,
    counts: [usize; 6],
}

impl LogClassifier {
    pub(crate) fn new() -> Self {
        Self {
            window: [0; 18],
            used: 0,
            counts: [0; 6],
        }
    }
    pub(crate) fn add(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if self.used == self.window.len() {
                self.window.rotate_left(1);
            } else {
                self.used += 1;
            }
            self.window[self.used - 1] = byte.to_ascii_lowercase();
            for (index, patterns) in PATTERNS.iter().enumerate() {
                if patterns
                    .iter()
                    .any(|pattern| self.window[..self.used].ends_with(pattern))
                {
                    self.counts[index] += 1;
                }
            }
        }
    }
    pub(crate) fn finish(mut self) -> Vec<LogSignalCount> {
        self.window.fill(0);
        [
            LogSignal::ConnectionRefused,
            LogSignal::Timeout,
            LogSignal::PermissionDenied,
            LogSignal::OutOfMemory,
            LogSignal::ErrorMarker,
            LogSignal::WarningMarker,
        ]
        .into_iter()
        .zip(self.counts)
        .map(|(signal, matches)| LogSignalCount { signal, matches })
        .collect()
    }
}

const PATTERNS: &[&[&[u8]]] = &[
    &[b"connection refused"],
    &[b"timeout", b"timed out"],
    &[b"permission denied"],
    &[b"out of memory", b"oom-kill"],
    &[b"error"],
    &[b"warn"],
];

#[cfg(test)]
mod tests {
    use super::{LogClassifier, LogSignal};
    #[test]
    fn matches_across_chunks_without_retaining_body() {
        let mut classifier = LogClassifier::new();
        for chunk in [
            b"PRIVATE connection re".as_slice(),
            b"fused TIMED ",
            b"OUT permission den",
            b"ied OOM-KILL ERROR WARNING PRIVATE",
        ] {
            classifier.add(chunk);
        }
        let counts = classifier.finish();
        assert!(counts.iter().all(|count| count.matches == 1));
        assert_eq!(counts[0].signal, LogSignal::ConnectionRefused);
        assert!(!serde_json::to_string(&counts)
            .expect("counts")
            .contains("PRIVATE"));
    }
}
