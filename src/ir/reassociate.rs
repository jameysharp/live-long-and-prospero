use std::mem::swap;
use std::num::Saturating;

use super::{BinOp, Inst, InstIdx, Insts, VarSet};

pub fn reassociate(insts: &mut Insts) {
    // for every commutative binary operator (op a b) we'll establish this invariant:
    // vars(a) == vars(b) implies one of these:
    //   b does not use the same operator
    //   a or b has multiple uses
    let mut state = State {
        vars: &mut insts.vars,
        uses: vec![Saturating(0u8); insts.pool.len()],
        stack: Vec::new(),
    };

    if let Some(last) = state.uses.last_mut() {
        last.0 = 1;
    }
    for (idx, inst) in insts.pool.iter().enumerate().rev() {
        if state.uses[idx].0 > 0 {
            for &arg in inst.args() {
                state.uses[arg.idx()] += 1;
            }
        }
    }

    for idx in 0..insts.pool.len() {
        let idx = InstIdx::try_from(idx).unwrap();
        if let Inst::BinOp { args, .. } = insts.pool[idx.idx()] {
            state.visit_binop(idx, args);
            while let Some(indexes) = state.stack.pop() {
                let mut subtree = insts
                    .pool
                    .get_disjoint_mut(indexes.map(InstIdx::idx))
                    .unwrap();
                debug_assert_eq!(subtree[0].args(), &indexes[1..]);

                state.rebalance_add_or_sub(&mut subtree);
                state.rebalance_commutative(subtree);
            }
        }
    }
}

struct State<'a> {
    vars: &'a mut [VarSet],
    uses: Vec<Saturating<u8>>,
    stack: Vec<[InstIdx; 3]>,
}

impl State<'_> {
    fn vars(&self, root: InstIdx) -> VarSet {
        self.vars[root.idx()]
    }

    fn can_reuse(&self, root: InstIdx) -> bool {
        self.uses[root.idx()].0 < 2
    }

    fn visit_binop(&mut self, root: InstIdx, [a, b]: [InstIdx; 2]) {
        // if both args are the same, their vars are equal, and we'd blow up
        // in get_disjoint_mut. can't reassociate that case without duplicating
        // instructions anyway.
        if a != b && self.vars(a) == self.vars(b) {
            self.stack.push([root, a, b]);
        }
    }

    fn rebalance_add_or_sub(&mut self, [inst, inst1, inst2]: &mut [&mut Inst; 3]) -> Option<()> {
        fn add_or_sub(inst: &Inst) -> Option<(bool, [InstIdx; 2])> {
            match *inst {
                Inst::BinOp { op, args } => match op {
                    BinOp::Add => Some((true, args)),
                    BinOp::Sub => Some((false, args)),
                    _ => None,
                },
                _ => None,
            }
        }

        let (sign1, [w, x]) = add_or_sub(inst1)?;
        let (sign2, [mut y, mut z]) = add_or_sub(inst2)?;

        // if both subterms are addition, use the general case
        if sign1 && sign2 {
            return None;
        }

        let (sign, [a, b]) = add_or_sub(inst)?;

        // if either subterm is a subtraction and has other uses, we can't rewrite
        // this subtree
        if !sign1 && !self.can_reuse(a) {
            return None;
        }
        if !sign2 && !self.can_reuse(b) {
            return None;
        }

        let (args, args1, args2) = if sign2 {
            debug_assert!(!sign1);
            if sign {
                // (w - x) + (y + z) = (w + (y + z)) - x
                ([a, x], Some([w, b]), None)
            } else {
                // (w - x) - (y + z) = w - (x + (y + z))
                ([w, a], Some([x, b]), None)
            }
        } else {
            if !sign {
                // a - (y - z) = a + (z - y)
                swap(&mut y, &mut z);
            }
            if sign1 {
                // (w + x) + (y - z) = ((w + x) + y) - z
                ([b, z], None, Some([a, y]))
            } else {
                // (w - x) + (y - z) = (w + y) - (x + z)
                ([a, b], Some([w, y]), Some([x, z]))
            }
        };

        // now rewrite subterms, adding them to the stack to revisit if changed
        let op = BinOp::Add;
        debug_assert_eq!(args1.is_none(), sign1);
        if let Some(args) = args1 {
            self.visit_binop(a, args);
            **inst1 = Inst::BinOp { op, args };
        }
        debug_assert_eq!(args2.is_none(), sign2);
        if let Some(args) = args2 {
            self.visit_binop(b, args);
            **inst2 = Inst::BinOp { op, args };
        }

        // and rewrite the root
        let op = BinOp::Sub;
        **inst = Inst::BinOp { op, args };

        Some(())
    }

    fn rebalance_commutative(&mut self, [inst, inst1, inst2]: [&mut Inst; 3]) {
        // rematch the (possibly rewritten) instruction under the new borrow
        let Inst::BinOp { op, args } = inst else {
            unreachable!()
        };

        if !op.is_commutative() {
            return;
        }

        let [a, b] = args;
        if let Some(args2) = inst2.is_binop_mut(*op) {
            if let Some(args1) = inst1.is_binop_mut(*op) {
                // don't modify inst1 or inst2 if they have other
                // uses besides in inst, and at this point we can't
                // reassociate without modifying both
                if self.can_reuse(*a) && self.can_reuse(*b) {
                    self.rebalance(args, args1, args2);
                }
            } else {
                // inst2 matched but inst1 did not, and they have the
                // same vars, so swap them to establish the invariant
                swap(a, b);
            }
        }
    }

    fn rebalance(
        &mut self,
        args: &mut [InstIdx; 2],
        args1: &mut [InstIdx; 2],
        args2: &mut [InstIdx; 2],
    ) {
        // we've matched a happy little balanced tree:
        //   inst(inst1(a1, b1), inst2(a2, b2))
        // all three instructions use the same operator and, inductively, inst1
        // and inst2 already satisfy our invariant.

        let [inst1, inst2] = *args;
        let [a1, b1] = *args1;
        let [a2, b2] = *args2;
        let leaves = [a1, a2, b1, b2].map(|idx| (self.vars(idx), idx));

        let mut insts = [(None, args), (Some(inst1), args1), (Some(inst2), args2)]
            .into_iter()
            .rev();
        let mut merge = |group: &mut Option<InstIdx>, new| {
            if let Some(old) = *group {
                let (inst, args) = insts.next().unwrap();
                let new_args = [old, new];
                *group = inst;
                if new_args != *args {
                    *args = new_args;
                    if let Some(inst) = inst {
                        let [a, b] = new_args;
                        self.vars[inst.idx()] = self.vars(a) | self.vars(b);
                        self.visit_binop(inst, new_args);
                    }
                }
            } else {
                *group = Some(new);
            }
        };

        let mut groups = [None; VarSet::ALL.idx() + 1];
        for (vars, idx) in leaves {
            merge(&mut groups[vars.idx()], idx);
        }

        let mut last = None;
        for group in groups.into_iter().rev() {
            if let Some(group) = group {
                merge(&mut last, group);
            }
        }
        debug_assert_eq!(last, None);
        debug_assert_eq!(insts.next(), None);
    }
}
