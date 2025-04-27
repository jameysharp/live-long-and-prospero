use std::mem::replace;

use crate::ir::{InstIdx, Location};

use super::{MemorySpace, Register};

// Modeled after https://www.mattkeeter.com/blog/2022-10-04-ssra/, except a
// value may be both in memory and in a register at the same time. That allows
// memory inputs and outputs for the function to be treated the same as stack
// spill-slots, as well as reducing the number of store instructions and, in my
// opinion, simplifying the implementation considerably.

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Allocation {
    reg: Option<Register>,
    mem: Option<MemorySpace>,
    loc: Location,
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
}

pub struct Registers<T> {
    allocs: Vec<Allocation>,
    recent: Lru,
    live: Vec<Option<InstIdx>>,
    stack_slots: Location,
    free_slots: Vec<(MemorySpace, Location)>,
    pub target: T,
}

impl<T: Target> Registers<T> {
    pub fn new(allocs: Vec<Allocation>, regs: usize, target: T) -> Self {
        Registers {
            allocs,
            recent: Lru::new(regs),
            live: vec![None; regs],
            stack_slots: 0,
            free_slots: Vec::new(),
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
            self.free_slots.push((mem, loc));
        }
        reg
    }

    pub fn get_reg(&mut self, idx: InstIdx) -> Register {
        // If this value already has a register allocated, return that.
        if let Some(reg) = self.allocs[idx.idx()].reg {
            debug_assert_eq!(Some(idx), self.live[reg.idx()]);
            self.recent.mark_used(reg);
            return reg;
        }

        // Otherwise, pick a register and hope nobody needs it too soon.
        let reg = Register::try_from(self.recent.pop()).unwrap();

        if let Some((mem, loc)) = self.clobber(idx, reg) {
            // Some later instruction wants this value in this register, so load
            // it for them.
            self.target.emit_load(reg, mem, loc);
        }

        reg
    }

    fn clobber(&mut self, idx: InstIdx, reg: Register) -> Option<(MemorySpace, u16)> {
        // Remember that this register now holds this value, and check what it
        // held before.
        self.allocs[idx.idx()].reg = Some(reg);

        // Was the selected register already holding another value?
        let live = self.live[reg.idx()].replace(idx)?;

        let alloc = &mut self.allocs[live.idx()];
        debug_assert_eq!(Some(reg), alloc.reg);

        // Make sure that value gets spilled, when we get to its definition,
        // by ensuring it has a memory location allocated.
        let (mem, loc) = if let Some(mem) = alloc.mem {
            (mem, alloc.loc)
        } else if let Some(slot) = self.free_slots.pop() {
            slot
        } else {
            let new_slot = self.stack_slots;
            self.stack_slots += 1;
            (MemorySpace::STACK, new_slot)
        };

        // Remember that this value is only in memory now.
        *alloc = Allocation {
            reg: None,
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
        if let Some(reg) = self.allocs[idx.idx()].reg {
            self.target.emit_load(reg, mem, loc);
            self.free_reg(reg);
        }
    }

    pub fn address_of(&self, idx: InstIdx) -> Option<(MemorySpace, Location)> {
        let Allocation { mem, loc, .. } = self.allocs[idx.idx()];
        Some((mem?, loc))
    }

    pub fn sink_load(&self, idx: InstIdx) -> bool {
        if let Allocation {
            reg: None,
            mem: Some(_),
            ..
        } = self.allocs[idx.idx()]
        {
            true
        } else {
            false
        }
    }

    pub fn finish(self) -> (T, Location) {
        (self.target, self.stack_slots)
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
