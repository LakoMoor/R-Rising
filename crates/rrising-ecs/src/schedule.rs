//! A deterministic system schedule over a [`World`]: ordered groups, a fixed-timestep frame step
//! and Unity-style change-version bookkeeping.
//!
//! Provenance: port-side design, not a translation of `ComponentSystemGroup`/`SystemState` (their
//! bodies were not re-read; see `docs/COMPATIBILITY.md`). It borrows the documented Unity
//! conventions only: systems run in a fixed order, each system remembers the
//! `GlobalSystemVersion` it last ran at, and a chunk "changed" for it when a component's chunk
//! change version is greater than that remembered version ([`FrameContext::did_change`]).
//!
//! Determinism rules: groups run in creation order; systems inside a group run in registration
//! order refined by `update_after` constraints (a stable topological sort, ties broken by
//! registration index); there is no hash-map iteration and no wall-clock input, time advances by
//! whole fixed steps of integer nanoseconds.
//!
//! Version protocol per system update: the system runs with `system_version = GlobalSystemVersion`
//! and `last_system_version` = the value it ran at last time (0 before the first run); its writes
//! are stamped `system_version`, so it does not see its own previous writes as changes, but any
//! write made afterwards (by a later system or by outside code) is stamped higher and is seen.
//! After the update the global version is incremented.
use std::any::Any;
use std::collections::BTreeSet;

use anyhow::{bail, ensure, Context, Result};

use crate::world::World;

/// What a system is told about the frame it runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameContext {
    /// 0-based fixed-step frame counter.
    pub frame: u64,
    /// Fixed step length in nanoseconds.
    pub delta_ns: u64,
    /// Simulated time at the start of this frame (`frame * delta_ns`).
    pub elapsed_ns: u64,
    /// `GlobalSystemVersion` this update runs at (writes are stamped with it).
    pub system_version: u32,
    /// The version this system last ran at; 0 before its first run.
    pub last_system_version: u32,
}

impl FrameContext {
    /// `ChangeVersionUtility.DidChange`: a chunk stamped `chunk_version` changed since this
    /// system last ran.
    pub fn did_change(&self, chunk_version: u32) -> bool {
        chunk_version > self.last_system_version
    }
}

/// A unit of simulation logic. `update` is called once per fixed-step frame.
pub trait System: Any {
    fn name(&self) -> &str;
    /// Names of systems in the same group that must run before this one.
    fn update_after(&self) -> Vec<String> {
        Vec::new()
    }
    fn update(&mut self, world: &mut World, ctx: &FrameContext) -> Result<()>;
    /// Boilerplate so callers can read a concrete system back ([`Schedule::system`]).
    fn as_any(&self) -> &dyn Any;
}

struct Entry {
    system: Box<dyn System>,
    last_version: u32,
}

struct Group {
    name: String,
    entries: Vec<Entry>,
    /// Execution order into `entries`, valid when `!dirty`.
    order: Vec<usize>,
    dirty: bool,
}

impl Group {
    fn resolve(&mut self) -> Result<()> {
        let n = self.entries.len();
        let names: Vec<&str> = self.entries.iter().map(|e| e.system.name()).collect();
        let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, e) in self.entries.iter().enumerate() {
            for a in e.system.update_after() {
                let j = names.iter().position(|n| *n == a).with_context(|| {
                    format!(
                        "system {:?} in group {:?} updates after unknown system {a:?}",
                        names[i], self.name
                    )
                })?;
                deps[i].push(j);
            }
        }
        let mut done = vec![false; n];
        let mut ready: BTreeSet<usize> = (0..n).filter(|&i| deps[i].is_empty()).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(&i) = ready.iter().next() {
            ready.remove(&i);
            done[i] = true;
            order.push(i);
            for k in 0..n {
                if !done[k] && !ready.contains(&k) && deps[k].iter().all(|&d| done[d]) {
                    ready.insert(k);
                }
            }
        }
        if order.len() != n {
            bail!("system ordering cycle in group {:?}", self.name);
        }
        self.order = order;
        self.dirty = false;
        Ok(())
    }
}

/// Ordered groups of systems with a fixed-timestep driver.
pub struct Schedule {
    delta_ns: u64,
    groups: Vec<Group>,
    frame: u64,
    accumulator_ns: u64,
}

impl Schedule {
    /// `delta_ns` is the fixed step in nanoseconds (must be non-zero).
    pub fn new(delta_ns: u64) -> Result<Self> {
        ensure!(delta_ns > 0, "fixed step must be non-zero");
        Ok(Self {
            delta_ns,
            groups: Vec::new(),
            frame: 0,
            accumulator_ns: 0,
        })
    }

    /// Append a group (groups run in creation order).
    pub fn add_group(&mut self, name: &str) -> Result<()> {
        ensure!(
            self.groups.iter().all(|g| g.name != name),
            "duplicate group {name:?}"
        );
        self.groups.push(Group {
            name: name.to_owned(),
            entries: Vec::new(),
            order: Vec::new(),
            dirty: true,
        });
        Ok(())
    }

    pub fn add_system(&mut self, group: &str, system: Box<dyn System>) -> Result<()> {
        let g = self
            .groups
            .iter_mut()
            .find(|g| g.name == group)
            .with_context(|| format!("unknown group {group:?}"))?;
        ensure!(
            g.entries.iter().all(|e| e.system.name() != system.name()),
            "duplicate system {:?} in group {group:?}",
            system.name()
        );
        g.entries.push(Entry {
            system,
            last_version: 0,
        });
        g.dirty = true;
        Ok(())
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn delta_ns(&self) -> u64 {
        self.delta_ns
    }

    /// `group/system` names in execution order.
    pub fn execution_order(&mut self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for g in &mut self.groups {
            if g.dirty {
                g.resolve()?;
            }
            for &i in &g.order {
                out.push(format!("{}/{}", g.name, g.entries[i].system.name()));
            }
        }
        Ok(out)
    }

    /// Read a registered system back as its concrete type.
    pub fn system<T: System>(&self, name: &str) -> Option<&T> {
        self.groups
            .iter()
            .flat_map(|g| &g.entries)
            .find(|e| e.system.name() == name)
            .and_then(|e| e.system.as_any().downcast_ref::<T>())
    }

    /// Run exactly one fixed-step frame.
    pub fn run_frame(&mut self, world: &mut World) -> Result<()> {
        let (frame, delta_ns) = (self.frame, self.delta_ns);
        for g in &mut self.groups {
            if g.dirty {
                g.resolve()?;
            }
            for k in 0..g.order.len() {
                let e = &mut g.entries[g.order[k]];
                let version = world.store.global_system_version();
                let ctx = FrameContext {
                    frame,
                    delta_ns,
                    elapsed_ns: frame * delta_ns,
                    system_version: version,
                    last_system_version: e.last_version,
                };
                e.system.update(world, &ctx).with_context(|| {
                    format!("system {:?} failed in frame {frame}", e.system.name())
                })?;
                e.last_version = version;
                world.store.increment_global_system_version();
            }
        }
        self.frame += 1;
        Ok(())
    }

    /// Accumulate `elapsed_ns` and run as many whole fixed steps as fit, at most `max_steps`
    /// (the remainder beyond the cap is dropped, as a spiral-of-death guard). Returns the steps run.
    pub fn advance(&mut self, world: &mut World, elapsed_ns: u64, max_steps: u32) -> Result<u32> {
        self.accumulator_ns = self.accumulator_ns.saturating_add(elapsed_ns);
        let mut steps = 0;
        while self.accumulator_ns >= self.delta_ns && steps < max_steps {
            self.accumulator_ns -= self.delta_ns;
            self.run_frame(world)?;
            steps += 1;
        }
        if steps == max_steps {
            self.accumulator_ns %= self.delta_ns;
        }
        Ok(steps)
    }
}
