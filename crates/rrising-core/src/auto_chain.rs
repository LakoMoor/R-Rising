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

#[derive(Clone, Copy, Debug)]
pub struct AutoProgressElement {
    pub enabled: bool,
    pub duration: f32,
    pub target: i32,
    pub end: bool,
    pub event: [u8; 32],
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoProgress {
    pub success: bool,
    pub origin: i32,
    pub target: i32,
    pub event: [u8; 32],
    pub forwarded: f64,
}
/// Iterative equivalent of the verified valid-array automatic progression path.
/// max_steps is a host safety budget; exhaustion is an error, not an original result.
#[allow(clippy::too_many_arguments)]
pub fn progress(
    elements: &[AutoProgressElement],
    main: i32,
    current: i32,
    loop_on_end: bool,
    stop_at_main: bool,
    skip_next_lifetime: bool,
    surplus: f64,
    max_steps: usize,
) -> anyhow::Result<AutoProgress> {
    let mut current = current;
    let mut stop = stop_at_main;
    let mut skip = skip_next_lifetime;
    let mut surplus = surplus;
    let mut last = None;
    for _ in 0..max_steps {
        let Some(element) = usize::try_from(current).ok().and_then(|i| elements.get(i)) else {
            return Ok(last.unwrap_or(AutoProgress {
                success: false,
                origin: 0,
                target: current,
                event: [0; 32],
                forwarded: 0.,
            }));
        };
        if (current == main && stop)
            || !element.enabled
            || (!skip && f64::from(element.duration) > surplus)
        {
            return Ok(last.unwrap_or(AutoProgress {
                success: false,
                origin: current,
                target: current,
                event: [0; 32],
                forwarded: surplus,
            }));
        }
        if !skip {
            surplus -= f64::from(element.duration);
        }
        let target = if element.end && !loop_on_end {
            -1
        } else {
            element.target
        };
        last = Some(AutoProgress {
            success: true,
            origin: current,
            target,
            event: element.event,
            forwarded: surplus,
        });
        if element.end && !loop_on_end {
            return Ok(last.expect("just assigned"));
        }
        stop |= current == main;
        current = element.target;
        skip = false;
    }
    anyhow::bail!("automatic progression exceeded host step budget")
}

/// Verified finite-input branch of the original forwarded-time remainder helper.
pub fn forwarded_remainder(value: f64, duration: f64) -> anyhow::Result<f64> {
    anyhow::ensure!(
        value.is_finite() && duration.is_finite() && duration != 0.,
        "remainder outside verified finite nonzero-divisor domain"
    );
    Ok(value % duration)
}
