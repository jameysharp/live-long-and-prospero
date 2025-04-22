use std::collections::HashMap;

use super::{BinOp, Const, Inst, InstIdx, InstSink, Location, UnOp, Var, VarSet};

pub struct Memoized {
    pub consts: Vec<Const>,
    pub funcs: [MemoizedFunc; VarSet::ALL.idx()],
}

#[derive(Default)]
pub struct MemoizedFunc {
    pub vars: VarSet,
    pub insts: Vec<Inst>,
    pub outputs: Vec<Option<InstIdx>>,
}

impl MemoizedFunc {
    fn push(&mut self, inst: Inst) -> InstIdx {
        let idx = InstIdx::try_from(self.insts.len()).unwrap();
        self.insts.push(inst);
        idx
    }

    fn add_output(&mut self, def: InstIdx) -> Location {
        let idx = self.outputs.len().try_into().unwrap();
        self.outputs.push(Some(def));
        idx
    }
}

pub struct MemoBuilder {
    result: Memoized,
    load: [HashMap<MemoIdx, InstIdx>; VarSet::ALL.idx()],
    store: [Vec<Location>; VarSet::ALL.idx()],
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MemoIdx {
    vars: VarSet,
    idx: Option<InstIdx>,
}

impl InstSink for MemoBuilder {
    type Idx = MemoIdx;
    type Output = Memoized;

    fn push_const(&mut self, value: Const) -> Self::Idx {
        let loc = self.result.consts.len().try_into().unwrap();
        self.result.consts.push(value);
        MemoIdx {
            vars: VarSet::default(),
            idx: Some(loc),
        }
    }

    fn push_var(&mut self, var: Var) -> Self::Idx {
        MemoIdx {
            vars: var.into(),
            idx: None,
        }
    }

    fn push_unop(&mut self, op: UnOp, arg: Self::Idx) -> Self::Idx {
        let vars = arg.vars;
        let arg = self.ensure_load(vars, arg);
        self.push(vars, Inst::UnOp { op, arg })
    }

    fn push_binop(&mut self, op: BinOp, [a, b]: [Self::Idx; 2]) -> Self::Idx {
        let vars = a.vars | b.vars;
        let args = [a, b].map(|arg| self.ensure_load(vars, arg));
        self.push(vars, Inst::BinOp { op, args })
    }

    fn push_load(&mut self, _vars: VarSet, _loc: Location) -> Self::Idx {
        todo!()
    }

    fn finish(mut self, last: Self::Idx) -> Self::Output {
        self.result.funcs[func_for(last.vars)].add_output(last.idx.unwrap());
        self.result
    }
}

impl MemoBuilder {
    pub fn new() -> Self {
        let mut funcs: [MemoizedFunc; VarSet::ALL.idx()] = Default::default();
        for (idx, func) in funcs.iter_mut().enumerate() {
            func.vars = VarSet((idx + 1) as u8);
        }
        for var in VarSet::ALL {
            funcs[func_for(var.into())].outputs.push(None);
        }

        Self {
            result: Memoized {
                consts: Vec::new(),
                funcs,
            },
            load: Default::default(),
            store: Default::default(),
        }
    }

    fn ensure_load(&mut self, vars: VarSet, arg: MemoIdx) -> InstIdx {
        let loc = if let Some(idx) = arg.idx {
            if arg.vars == vars {
                return idx;
            }
            if let Some(arg_func) = arg.vars.idx().checked_sub(1) {
                let location = &mut self.store[arg_func][idx.idx()];
                if *location == Location::MAX {
                    *location = self.result.funcs[arg_func].add_output(idx);
                }
                *location
            } else {
                idx.idx().try_into().unwrap()
            }
        } else {
            0
        };
        let func_idx = func_for(vars);
        *self.load[func_idx].entry(arg).or_insert_with(|| {
            let vars = arg.vars;
            self.store[func_idx].push(Location::MAX);
            self.result.funcs[func_idx].push(Inst::Load { vars, loc })
        })
    }

    fn push(&mut self, vars: VarSet, inst: Inst) -> MemoIdx {
        let func_idx = func_for(vars);
        self.store[func_idx].push(Location::MAX);
        let idx = Some(self.result.funcs[func_idx].push(inst));
        MemoIdx { vars, idx }
    }
}

fn func_for(vars: VarSet) -> usize {
    if let Some(func_idx) = vars.idx().checked_sub(1) {
        func_idx
    } else {
        todo!("constant folding");
    }
}
