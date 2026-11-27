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

#[derive(Clone, Copy, Debug)]
pub struct AutoPathStep {
    pub enabled: bool,
    pub duration: f32,
    pub target: i32,
}
/// Bounded valid-array path of TryCalculateDurationUntilMainElement.
/// Invalid array access is represented as None; original exception behavior is excluded.
pub fn duration_until_main(elements: &[AutoPathStep], main_index: i32, start: i32) -> Option<f64> {
    let mut current = start;
    let mut duration = 0.;
    for _ in 0..20 {
        let index = usize::try_from(current).ok()?;
        if index > elements.len() {
            return None;
        }
        if current == main_index {
            return Some(duration);
        }
        let step = elements.get(index)?;
        if !step.enabled {
            return None;
        }
        duration += f64::from(step.duration);
        current = step.target;
    }
    None
}
#[cfg(feature = "content")]
pub fn definition_duration_until_main(
    chain: &rrising_content::spawn_chain::ChainDefinition,
    start: i32,
) -> Option<f64> {
    let elements: Vec<_> = chain
        .elements
        .iter()
        .map(|element| AutoPathStep {
            enabled: element.use_auto_transition,
            duration: element.auto_transition.duration,
            target: usize::try_from(element.auto_transition.transition_index)
                .ok()
                .and_then(|index| element.transitions.get(index))
                .map_or(-1, |transition| transition.target),
        })
        .collect();
    duration_until_main(&elements, chain.main_index, start)
}
