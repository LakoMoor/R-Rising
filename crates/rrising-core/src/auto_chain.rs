//! Auto-chain clock helpers verified against isolated pinned client functions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoChainClock {
    pub next_attempt: f64,
}
impl AutoChainClock {
    pub fn create(
        enabled: bool,
        duration: f32,
        server_seconds: f64,
        forwarded_seconds: f64,
    ) -> Self {
        let next_attempt = if enabled {
            let next = f64::from(duration) + server_seconds;
            if forwarded_seconds > 0. {
                next - forwarded_seconds
            } else {
                next
            }
        } else {
            -1.
        };
        Self { next_attempt }
    }
    pub fn remaining(self, server_seconds: f64) -> f64 {
        self.next_attempt - server_seconds
    }
    pub fn passed(self, duration: f32, server_seconds: f64) -> f64 {
        f64::from(duration) - (self.next_attempt - server_seconds)
    }
}
