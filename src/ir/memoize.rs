use std::collections::HashMap;

use super::{Const, Inst, InstIdx, Insts, VarSet};

pub struct Memoized {
    pub consts: Vec<Const>,
    pub funcs: [MemoizedFunc; VarSet::ALL.idx()],
}

#[derive(Default)]
pub struct MemoizedFunc {
    pub vars: VarSet,
    pub insts: Vec<Inst>,
    pub location: Vec<(InstIdx, VarSet, InstIdx)>,
    pub outputs: InstIdx,
}

impl MemoizedFunc {
    fn push(&mut self, inst: Inst) -> InstIdx {
        let idx = InstIdx::try_from(self.insts.len()).unwrap();
        self.insts.push(inst);
        idx
    }

    fn add_output(&mut self, def: InstIdx) -> InstIdx {
        let idx = self.outputs;
        self.outputs = idx.checked_add(1).unwrap();
        self.location.push((def, self.vars, idx));
        idx
    }
}

pub fn memoize(insts: &Insts) -> Memoized {
    let mut funcs: [MemoizedFunc; VarSet::ALL.idx() + 1] = Default::default();
    for (idx, func) in funcs.iter_mut().enumerate() {
        func.vars = VarSet(idx as u8);
    }

    let mut location: Vec<InstIdx> = vec![InstIdx::MAX; insts.len()];
    let mut remap: HashMap<(VarSet, InstIdx), InstIdx> = HashMap::new();

    for (idx, inst) in insts.iter().enumerate() {
        let idx = InstIdx::try_from(idx).unwrap();
        let vars = insts.vars(idx);

        let mut inst = inst.clone();
        for arg in inst.args_mut() {
            let arg_vars = insts.vars(*arg);
            let arg_def = remap[&(arg_vars, *arg)];
            *arg = *remap.entry((vars, *arg)).or_insert_with(|| {
                // if we haven't already remapped this argument, then it's
                // from a different varset and we need to load it now
                assert_ne!(vars, arg_vars);

                let location = &mut location[usize::from(*arg)];
                if *location == InstIdx::MAX {
                    // we haven't yet added this arg to the outputs of the
                    // function that computes it, so do that first
                    *location = funcs[arg_vars.idx()].add_output(arg_def);
                }

                let new_idx = funcs[vars.idx()].push(Inst::Load);
                funcs[vars.idx()]
                    .location
                    .push((new_idx, arg_vars, *location));
                new_idx
            });
        }

        let new_idx = funcs[vars.idx()].push(inst);
        remap.insert((vars, idx), new_idx);
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
        .location
        .into_iter()
        .enumerate()
        .map(|(expected_loc, (idx, _, loc))| {
            let Inst::Const { value } = consts.insts[usize::from(idx)] else {
                todo!("constant folding")
            };
            debug_assert_eq!(loc, expected_loc as InstIdx);
            value
        })
        .collect();

    Memoized { consts, funcs }
}
