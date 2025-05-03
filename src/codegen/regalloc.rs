use clap::{Args, ValueEnum};
use std::mem::replace;

use crate::ir::{InstIdx, Location};

use super::{MemorySpace, Register};

// Modeled after https://www.mattkeeter.com/blog/2022-10-04-ssra/, except a
// value may be both in memory and in a register at the same time. That allows
// memory inputs and outputs for the function to be treated the same as stack
// spill-slots, as well as reducing the number of store instructions and, in my
// opinion, simplifying the implementation considerably.

#[derive(Args, Clone, Copy, Debug, Default)]
pub struct Config {
    /// On some architectures, arithmetic instructions can also load a value from
    /// memory without needing to place it in a register first. This reduces
    /// register pressure, at the cost of potentially duplicating loads.
    #[arg(long, default_value_t = SinkLoads::default(), value_enum)]
    pub sink_loads: SinkLoads,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum SinkLoads {
    /// Don't sink loads
    None,
    /// Sink loads unless a register is available, even if that register
    /// requires spilling live values
    #[default]
    SpillAny,
    /// Sink loads unless a register is available, preferring registers which
    /// don't require spilling live values
    PreferDead,
    /// Sink loads unless a register is available without spilling live values
    RequireDead,
    /// Sink all loads when possible
    All,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Allocation {
    reg: RegisterState,
    mem: Option<MemorySpace>,
    loc: Location,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum RegisterState {
    #[default]
    Unallocated,
    Reg(Register),
    SunkLoad(DirtyPoolIndex),
}

impl Allocation {
    pub fn initial_location(&mut self, mem: MemorySpace, loc: Location) {
        assert_eq!(self, &Allocation::default());
        self.mem = Some(mem);
        self.loc = loc;
    }
}

pub trait Target {
    fn emit_load(&mut self, reg: Register, mem: MemorySpace, loc: Location);
    fn emit_store(&mut self, reg: Register, mem: MemorySpace, loc: Location);
    fn patch_sunk_load(
        &mut self,
        patch_at: usize,
        reg: Register,
        other: Option<(MemorySpace, Location)>,
    );
}

pub struct Registers<T> {
    config: Config,
    allocs: Vec<Allocation>,
    recent: Lru,
    live: Vec<Option<InstIdx>>,
    dirty_pool: DirtyPool,
    stack_slots: Location,
    free_slots: Vec<(u16, MemorySpace, Location)>,
    free_generation: u16,
    pub target: T,
}

impl<T: Target> Registers<T> {
    pub fn new(config: Config, allocs: Vec<Allocation>, regs: usize, target: T) -> Self {
        Registers {
            config,
            allocs,
            recent: Lru::new(regs),
            live: vec![None; regs],
            dirty_pool: DirtyPool::new(regs),
            stack_slots: 0,
            free_slots: Vec::new(),
            free_generation: 0,
            target,
        }
    }

    pub fn get_output_reg(&mut self, idx: InstIdx) -> Register {
        let reg = self.get_reg(idx);
        self.free_reg(reg);
        if let Allocation {
            mem: Some(mem),
            loc,
            ..
        } = self.allocs[idx.idx()]
        {
            self.target.emit_store(reg, mem, loc);
            // Any place we're going to store to, not just stack slots, can be
            // safely used as a spill slot for earlier instructions.
            self.free_slots.push((self.free_generation, mem, loc));
            self.free_generation += 1;
        }
        reg
    }

    pub fn get_reg(&mut self, idx: InstIdx) -> Register {
        // If this value already has a register allocated, return that.
        if let Some(reg) = self.float_load(idx) {
            debug_assert_eq!(Some(idx), self.live[reg.idx()]);
            self.recent.mark_used(reg);
            self.dirty_pool.mark_dirty(reg);
            return reg;
        }

        // Otherwise, pick a register and hope nobody needs it too soon.
        let reg = Register::try_from(self.recent.pop()).unwrap();

        if let Some((mem, loc)) = self.clobber(idx, reg, self.free_generation) {
            // Some later instruction wants this value in this register, so load
            // it for them.
            self.target.emit_load(reg, mem, loc);
        }

        reg
    }

    fn clobber(
        &mut self,
        idx: InstIdx,
        reg: Register,
        free_generation: u16,
    ) -> Option<(MemorySpace, u16)> {
        // Remember that this register now holds this value, and check what it
        // held before.
        self.allocs[idx.idx()].reg = RegisterState::Reg(reg);
        self.dirty_pool.mark_dirty(reg);

        // Was the selected register already holding another value?
        let live = self.live[reg.idx()].replace(idx)?;

        let alloc = &mut self.allocs[live.idx()];
        debug_assert_eq!(RegisterState::Reg(reg), alloc.reg);

        // Make sure that value gets spilled, when we get to its definition,
        // by ensuring it has a memory location allocated.
        let (mem, loc) = if let Some(mem) = alloc.mem {
            (mem, alloc.loc)
        } else if let Some(pos) = self
            .free_slots
            .iter()
            .rposition(|&(generation, _, _)| generation < free_generation)
        {
            let (_, mem, loc) = self.free_slots.swap_remove(pos);
            (mem, loc)
        } else {
            let new_slot = self.stack_slots;
            self.stack_slots += 1;
            (MemorySpace::STACK, new_slot)
        };

        // Remember that this value is only in memory now.
        *alloc = Allocation {
            reg: RegisterState::Unallocated,
            mem: Some(mem),
            loc,
        };
        Some((mem, loc))
    }

    fn free_reg(&mut self, reg: Register) {
        self.recent.mark_unused(reg);
        self.live[reg.idx()] = None;
    }

    // A load instruction is special. We never store its value, because it's
    // already in memory. And if it doesn't have a register assigned to it at
    // this point, that means no instruction needs the load to happen here, so
    // we can skip it. That could happen either if the result of the load is
    // never used (unlikely) or if, sometime between allocating an instruction
    // that uses this value and now, another instruction needed a register and
    // stole the one we'd have used. But in that case, the load is emitted at
    // that time, so we have nothing to do now.
    pub fn emit_load(&mut self, idx: InstIdx, mem: MemorySpace, loc: Location) {
        if let RegisterState::Reg(reg) = self.allocs[idx.idx()].reg {
            self.dirty_pool.mark_dirty(reg);
            self.target.emit_load(reg, mem, loc);
            self.free_reg(reg);
        }
    }

    pub fn address_of(&self, idx: InstIdx) -> Option<(MemorySpace, Location)> {
        let Allocation { mem, loc, .. } = self.allocs[idx.idx()];
        Some((mem?, loc))
    }

    pub fn sink_load(&mut self, idx: InstIdx, patch_at: usize) -> bool {
        if self.config.sink_loads != SinkLoads::None && self.float_load(idx).is_none() {
            match self.config.sink_loads {
                SinkLoads::None | SinkLoads::All => {}
                SinkLoads::RequireDead | SinkLoads::PreferDead | SinkLoads::SpillAny => {
                    let pool_idx = self
                        .dirty_pool
                        .push_load(idx, patch_at, self.free_generation);
                    self.allocs[idx.idx()].reg = RegisterState::SunkLoad(pool_idx);
                }
            }
            true
        } else {
            false
        }
    }

    fn float_load(&mut self, idx: InstIdx) -> Option<Register> {
        let reg = &mut self.allocs[idx.idx()].reg;
        match *reg {
            RegisterState::Unallocated => None,
            RegisterState::Reg(register) => Some(register),
            RegisterState::SunkLoad(pool_idx) => {
                let (mut clean_regs, free_generation, patch_at) =
                    self.dirty_pool.get_clean_regs(pool_idx, idx);

                match self.config.sink_loads {
                    SinkLoads::PreferDead => {
                        let dead_regs = clean_regs & dead_regs(&self.live);
                        if dead_regs != 0 {
                            clean_regs = dead_regs;
                        }
                    }
                    SinkLoads::RequireDead => clean_regs &= dead_regs(&self.live),
                    _ => {}
                }

                if clean_regs == 0 {
                    *reg = RegisterState::Unallocated;
                    None
                } else {
                    let clean_reg = self.recent.pop_first_in(clean_regs);
                    let other = self.clobber(idx, clean_reg, free_generation);
                    self.target.patch_sunk_load(patch_at, clean_reg, other);
                    Some(clean_reg)
                }
            }
        }
    }

    pub fn finish(self) -> (T, Location) {
        (self.target, self.stack_slots)
    }
}

fn dead_regs(items: &Vec<Option<InstIdx>>) -> u32 {
    let mut dead_regs = 0;
    for (reg, live) in items.iter().enumerate() {
        if live.is_none() {
            dead_regs |= 1 << reg;
        }
    }
    dead_regs
}

#[derive(Clone, Copy, Default)]
struct QueuedLoad {
    inst: Option<InstIdx>,
    free_generation: u16,
}

// Most of the time we can only find a register to float a load pretty soon
// after the load; on the Prospero challenge with 15 registers, usually there
// are only registers that are clean and not live within a couple of loads,
// and the longest was 27 loads later. So allowing up to 32 loads in the queue
// is generous.
struct DirtyPool {
    loads: [QueuedLoad; 32],
    patch_at: [usize; 32],
    front: usize,
    dirty_before: Vec<usize>,
}

type DirtyPoolIndex = u8;

impl DirtyPool {
    fn new(regs: usize) -> Self {
        Self {
            loads: Default::default(),
            patch_at: Default::default(),
            front: 0,
            dirty_before: vec![0; regs],
        }
    }

    fn push_load(
        &mut self,
        load: InstIdx,
        patch_at: usize,
        free_generation: u16,
    ) -> DirtyPoolIndex {
        let idx = self.front % self.loads.len();
        self.front += 1;
        self.loads[idx] = QueuedLoad {
            inst: Some(load),
            free_generation,
        };
        self.patch_at[idx] = patch_at;
        idx.try_into().unwrap()
    }

    fn mark_dirty(&mut self, i: Register) {
        self.dirty_before[i.idx()] = self.front;
    }

    fn get_clean_regs(&self, idx: DirtyPoolIndex, load: InstIdx) -> (u32, u16, usize) {
        let mut idx = usize::from(idx);
        let queued_load = self.loads[idx];
        if queued_load.inst != Some(load) {
            return (0, 0, 0);
        }
        let patch_at = self.patch_at[idx];

        assert!(self.loads.len().is_power_of_two());
        idx |= self.front & !(self.loads.len() - 1);
        if idx >= self.front {
            idx -= self.loads.len();
        }

        let mut result = 0;
        for (reg, &dirty_before) in self.dirty_before.iter().enumerate() {
            if idx >= dirty_before {
                result |= 1 << reg;
            }
        }
        (result, queued_load.free_generation, patch_at)
    }
}

struct Lru {
    data: Vec<LruNode>,
    head: Register,
}

struct LruNode {
    prev: Register,
    next: Register,
}

impl Lru {
    pub fn new(len: usize) -> Self {
        Self {
            data: (0..len)
                .map(|i| LruNode {
                    prev: ((i + len - 1) % len).try_into().unwrap(),
                    next: ((i + 1) % len).try_into().unwrap(),
                })
                .collect(),
            head: 0.try_into().unwrap(),
        }
    }

    /// Mark the given node as newest
    pub fn mark_used(&mut self, i: Register) {
        self.mark_unused(i);
        self.head = i;
    }

    /// Mark the given node as oldest
    pub fn mark_unused(&mut self, i: Register) {
        if i == self.head {
            self.head = self.data[i.idx()].next;
            return;
        }

        // If this wasn't the oldest node, then remove it and
        // reinsert it right before the head of the list.
        let next = self.head;
        let prev = replace(&mut self.data[next.idx()].prev, i);
        if prev != i {
            self.data[prev.idx()].next = i;
            let LruNode { prev, next } = replace(&mut self.data[i.idx()], LruNode { next, prev });
            self.data[prev.idx()].next = next;
            self.data[next.idx()].prev = prev;
        }
    }

    /// Look up the oldest node in the list, marking it as newest
    pub fn pop(&mut self) -> Register {
        let out = self.data[self.head.idx()].prev;
        self.head = out; // rotate so that oldest becomes newest
        out
    }

    pub fn pop_first_in(&mut self, free_regs: u32) -> Register {
        let mut i = self.head;
        loop {
            i = self.data[i.idx()].prev;
            if free_regs & (1 << i.idx()) != 0 {
                self.mark_used(i);
                return i;
            }
            if i == self.head {
                unreachable!();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tiny_lru() {
        let mut lru = Lru::new(2);
        lru.mark_used(0.try_into().unwrap());
        assert_eq!(lru.pop().idx(), 1);
        assert_eq!(lru.pop().idx(), 0);

        lru.mark_used(1.try_into().unwrap());
        assert_eq!(lru.pop().idx(), 0);
        assert_eq!(lru.pop().idx(), 1);
    }

    #[test]
    fn test_medium_lru() {
        let mut lru = Lru::new(10);
        lru.mark_used(0.try_into().unwrap());
        for _ in 0..9 {
            assert_ne!(lru.pop().idx(), 0);
        }
        assert_eq!(lru.pop().idx(), 0);

        lru.mark_used(1.try_into().unwrap());
        for _ in 0..9 {
            assert_ne!(lru.pop().idx(), 1);
        }
        assert_eq!(lru.pop().idx(), 1);

        lru.mark_used(4.try_into().unwrap());
        lru.mark_used(5.try_into().unwrap());
        for _ in 0..8 {
            assert!(!matches!(lru.pop().idx(), 4 | 5));
        }
        assert_eq!(lru.pop().idx(), 4);
        assert_eq!(lru.pop().idx(), 5);
    }
}
