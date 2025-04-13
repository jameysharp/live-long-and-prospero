use std::collections::HashMap;

use super::{Const, Inst, InstIdx, Insts, VarSet};

#[derive(Default)]
pub struct Memoized {
    pub consts: Vec<Const>,
    pub funcs: [MemoizedFunc; VarSet::ALL.idx()],
}

#[derive(Default)]
pub struct MemoizedFunc {
    pub insts: Vec<Inst>,
    pub location: Vec<(InstIdx, VarSet, InstIdx)>,
    pub outputs: InstIdx,
}

impl MemoizedFunc {
    fn push(&mut self, inst: Inst, vars: VarSet, location: InstIdx) -> InstIdx {
        let idx = InstIdx::try_from(self.insts.len()).unwrap();
        self.insts.push(inst);
        if location != InstIdx::MAX {
            self.location.push((idx, vars, location));
        }
        idx
    }
}

pub fn memoize(insts: &Insts) -> Memoized {
    let mut counters = Counters::default();
    counters.location.resize(insts.len(), InstIdx::MAX);

    for (idx, inst) in insts.iter().enumerate() {
        let idx = InstIdx::try_from(idx).unwrap();
        let vars = insts.vars(idx);

        // need space for this instruction
        counters.inst(vars);

        // find any args which should be memoized
        for &arg in inst.args() {
            let arg_vars = insts.vars(arg);
            if arg_vars != vars {
                // might need a load instruction
                counters.inst(vars);
                counters.memoize(arg, arg_vars);
            }
        }
    }

    // mark the last result as needing to be written too
    let last = InstIdx::try_from(insts.len().checked_sub(1).unwrap()).unwrap();
    counters.memoize(last, insts.vars(last));

    let mut out = Memoized::default();
    out.consts
        .resize(usize::from(counters.storage[0]), Const::default());
    for ((func, &capacity), &storage) in out
        .funcs
        .iter_mut()
        .zip(&counters.insts[1..])
        .zip(&counters.storage[1..])
    {
        func.insts.reserve(capacity.into());
        func.location.reserve(storage.into());
        func.outputs = storage;
    }

    let mut remap = HashMap::new();
    for (idx, inst) in insts.iter().enumerate() {
        let idx = InstIdx::try_from(idx).unwrap();
        let vars = insts.vars(idx);

        if let Some(func) = vars.idx().checked_sub(1) {
            let mut inst = inst.clone();
            for arg in inst.args_mut() {
                *arg = *remap.entry((func, *arg)).or_insert_with(|| {
                    // if we haven't already remapped this argument, then it's
                    // from a different varset and we need to load it now
                    let arg_vars = insts.vars(*arg);
                    assert_ne!(vars, arg_vars);
                    out.funcs[func].push(Inst::Load, arg_vars, counters.location[usize::from(*arg)])
                });
            }

            let new_idx = out.funcs[func].push(inst, vars, counters.location[usize::from(idx)]);
            remap.insert((func, idx), new_idx);
        } else {
            let Inst::Const { value } = inst else {
                todo!("constant folding")
            };
            out.consts[usize::from(counters.location[usize::from(idx)])] = *value;
        }
    }

    out
}

#[derive(Default)]
struct Counters {
    insts: [InstIdx; VarSet::ALL.idx() + 1],
    storage: [InstIdx; VarSet::ALL.idx() + 1],
    location: Vec<InstIdx>,
}

impl Counters {
    fn inst(&mut self, vars: VarSet) {
        self.insts[vars.idx()] += 1;
    }

    fn memoize(&mut self, idx: InstIdx, vars: VarSet) {
        // ensure storage has been allocated for memoizing this arg
        let location = &mut self.location[usize::from(idx)];
        if *location == InstIdx::MAX {
            let storage = &mut self.storage[vars.idx()];
            *location = *storage;
            *storage += 1;
        }
    }
}
