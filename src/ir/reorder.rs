use super::{Inst, InstIdx, Insts, VarSet};

pub fn reorder(insts: &mut Insts) {
    let Some(root) = insts.len().checked_sub(1) else {
        return;
    };
    insts.gvn.clear();

    let mut placed = 0;
    let mut remap = vec![InstIdx::INVALID; insts.len()];
    let mut stack = vec![InstIdx::try_from(root).unwrap()];
    while let Some(&idx) = stack.last() {
        let idx = idx.idx();
        if remap[idx] == InstIdx::INVALID {
            let mut changed = false;
            for &arg in insts[idx].args().iter().rev() {
                if remap[arg.idx()] == InstIdx::INVALID {
                    stack.push(arg);
                    changed = true;
                }
            }
            if changed {
                continue;
            }

            remap[idx] = placed.try_into().unwrap();
            placed += 1;
        }
        stack.pop();
    }
    drop(stack);

    let placed = usize::from(placed);
    let mut pool = vec![Inst::Load; placed];
    let mut vars = vec![VarSet::default(); placed];
    for (old, &new) in remap.iter().enumerate() {
        if new != InstIdx::INVALID {
            let new = new.idx();
            let mut inst = insts.pool[old].clone();
            for arg in inst.args_mut() {
                *arg = remap[arg.idx()];
            }
            pool[new] = inst;
            vars[new] = insts.vars[old];
        }
    }
    insts.pool = pool;
    insts.vars = vars;
}
