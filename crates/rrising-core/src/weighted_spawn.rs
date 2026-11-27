//! Weighted spawn-chain choice, bounded to finite weights and valid array selection.
//! Scheduling, RNG initialization and exception paths are not implemented here.
use anyhow::{ensure, Context, Result};
pub struct WeightedChain {
    pub guid: i32,
    pub weight: f32,
}
pub struct WeightedGroup {
    pub chains: Vec<WeightedChain>,
    pub weight: f32,
    pub total_chain_weight: f32,
}
fn next_float(state: &mut u32) -> f32 {
    let before = *state;
    let mut next = before ^ (before << 13);
    next ^= next >> 17;
    next ^= next << 5;
    *state = next;
    f32::from_bits((before >> 9) | 0x3f800000) - 1.0
}
fn choose(weights: impl Iterator<Item = f32>, target: f32) -> Option<usize> {
    let mut sum = 0.;
    for (index, weight) in weights.enumerate() {
        sum += weight;
        if sum >= target {
            return Some(index);
        }
    }
    None
}
pub fn select(groups: &[WeightedGroup], total_group_weight: f32, state: &mut u32) -> Result<i32> {
    ensure!(
        total_group_weight.is_finite()
            && groups.iter().all(|g| g.weight.is_finite()
                && g.total_chain_weight.is_finite()
                && g.chains.iter().all(|c| c.weight.is_finite())),
        "nonfinite weight is outside verified selection path"
    );
    if groups.is_empty() {
        return Ok(0);
    }
    let target = next_float(state) * total_group_weight;
    let group = &groups[choose(groups.iter().map(|g| g.weight), target)
        .context("group selection outside array")?];
    if group.chains.is_empty() {
        return Ok(0);
    }
    let target = next_float(state) * group.total_chain_weight;
    let index = choose(group.chains.iter().map(|c| c.weight), target)
        .context("chain selection outside array")?;
    Ok(group.chains[index].guid)
}
