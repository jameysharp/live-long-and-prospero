use std::collections::HashMap;

use super::{Const, Inst, InstIdx, Insts, Location, VarSet};

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

pub fn memoize(insts: &Insts) -> Memoized {
    let mut funcs: [MemoizedFunc; VarSet::ALL.idx() + 1] = Default::default();
    for (idx, func) in funcs.iter_mut().enumerate() {
        func.vars = VarSet(idx as u8);
    }

    for var in VarSet::ALL {
        funcs[VarSet::from(var).idx()].outputs.push(None);
    }

    let mut location: Vec<Location> = vec![Location::MAX; insts.len()];
    let mut remap: HashMap<(VarSet, InstIdx), InstIdx> = HashMap::new();

    for (idx, inst) in insts.iter().enumerate() {
        let idx = InstIdx::try_from(idx).unwrap();
        let result_vars = insts.vars(idx);

        let inst = if let Inst::Var { .. } = inst {
            location[idx.idx()] = 0;
            Inst::Load {
                vars: result_vars,
                loc: 0,
            }
        } else {
            let mut inst = inst.clone();
            for arg in inst.args_mut() {
                let vars = insts.vars(*arg);
                let arg_def = remap[&(vars, *arg)];
                *arg = *remap.entry((result_vars, *arg)).or_insert_with(|| {
                    // if we haven't already remapped this argument, then it's
                    // from a different varset and we need to load it now
                    assert_ne!(result_vars, vars);

                    let loc = &mut location[arg.idx()];
                    if *loc == Location::MAX {
                        // we haven't yet added this arg to the outputs of the
                        // function that computes it, so do that first
                        *loc = funcs[vars.idx()].add_output(arg_def);
                    }

                    funcs[result_vars.idx()].push(Inst::Load { vars, loc: *loc })
                });
            }
            inst
        };

        let new_idx = funcs[result_vars.idx()].push(inst);
        remap.insert((result_vars, idx), new_idx);
    }

    // mark the last result as needing to be written too
    for func in funcs.iter_mut().rev() {
        if let Some(last) = func.insts.len().checked_sub(1) {
            func.add_output(InstIdx::try_from(last).unwrap());
            break;
        }
    }

    let [consts, funcs @ ..] = funcs;
    let consts = consts
        .outputs
        .into_iter()
        .map(|idx| {
            let Inst::Const { value } = consts.insts[idx.unwrap().idx()] else {
                todo!("constant folding")
            };
            value
        })
        .collect();

    Memoized { consts, funcs }
}
