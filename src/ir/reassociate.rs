use std::cmp::Ordering;
use std::mem::swap;
use std::num::Saturating;

use super::{BinOp, Inst, InstIdx, Insts, UnOp, VarSet};

pub fn reassociate(insts: &mut Insts) {
    // for every commutative binary operator (op a b) we'll establish this invariant:
    //   vars(a) <= vars(b)
    // further, vars(a) == vars(b) implies one of these:
    //   a does not use the same operator
    //   a or b has multiple uses
    let mut uses = vec![Saturating(0u8); insts.pool.len()];
    if let Some(last) = uses.last_mut() {
        last.0 = 1;
    }
    for (idx, inst) in insts.pool.iter().enumerate().rev() {
        if uses[idx].0 > 0 {
            for &arg in inst.args() {
                uses[usize::from(arg)] += 1;
            }
        }
    }

    let mut stack = Vec::new();
    for idx in 0..InstIdx::try_from(insts.len()).unwrap() {
        stack.push(idx);
        while let Some(root) = stack.pop() {
            if let Inst::BinOp { op, args: [a, b] } = &mut insts.pool[usize::from(root)] {
                if a == b {
                    // if both args are the same, their vars are equal, and we'd
                    // blow up in get_disjoint_mut below. can't reassociate this
                    // case without duplicating instructions anyway.
                    continue;
                }

                match insts.vars[usize::from(*a)].cmp(insts.vars[usize::from(*b)]) {
                    Ordering::Equal => simplify(*op, [root, *a, *b], &mut stack, &uses, insts),
                    Ordering::Greater if op.is_commutative() => swap(a, b),
                    _ => {}
                }
            }
        }
    }
}

fn simplify(
    op: BinOp,
    indexes: [InstIdx; 3],
    stack: &mut Vec<InstIdx>,
    uses: &[Saturating<u8>],
    insts: &mut Insts,
) {
    fn subtree(indexes: [InstIdx; 3], insts: &mut [Inst]) -> [&mut Inst; 3] {
        insts.get_disjoint_mut(indexes.map(usize::from)).unwrap()
    }

    let [r, a, b] = indexes;
    let [ir, ia, ib] = subtree(indexes, &mut insts.pool);
    let reuse_a = uses[usize::from(a)].0 < 2;
    let reuse_b = uses[usize::from(b)].0 < 2;

    let replace = |op, args| Some((Inst::BinOp { op, args }, r, args));

    let mut negate = |op, arg, iarg: &mut Inst, args| {
        *iarg = Inst::BinOp { op, args };
        insts.vars[usize::from(arg)] = insts.vars[usize::from(r)];
        Some((Inst::UnOp { op: UnOp::Neg, arg }, arg, args))
    };

    let indexes = match (op, ia.is_unop(UnOp::Neg), ib.is_unop(UnOp::Neg)) {
        // (-x) + y = y - x
        (BinOp::Add, Some(na), None) => replace(BinOp::Sub, [b, na]),
        // x + (-y) = x - y
        (BinOp::Add, None, Some(nb)) => replace(BinOp::Sub, [a, nb]),
        // (-x) + (-y) = -(x + y)
        (BinOp::Add, Some(na), Some(nb)) if reuse_a => negate(BinOp::Add, a, ia, [na, nb]),
        (BinOp::Add, Some(na), Some(nb)) if reuse_b => negate(BinOp::Add, b, ib, [na, nb]),

        // (-x) - y = -(x + y)
        (BinOp::Sub, Some(na), None) if reuse_a => negate(BinOp::Add, a, ia, [na, b]),
        // x - (-y) = x + y
        (BinOp::Sub, None, Some(nb)) => replace(BinOp::Add, [a, nb]),
        // (-x) - (-y) = y - x
        (BinOp::Sub, Some(na), Some(nb)) => replace(BinOp::Sub, [nb, na]),

        // (-x) * y = -(x * y)
        (BinOp::Mul, Some(na), None) if reuse_a => negate(BinOp::Mul, a, ia, [na, b]),
        // x * (-y) = -(x * y)
        (BinOp::Mul, None, Some(nb)) if reuse_b => negate(BinOp::Mul, b, ib, [a, nb]),
        // (-x) * (-y) = x * y
        (BinOp::Mul, Some(na), Some(nb)) => replace(BinOp::Mul, [na, nb]),

        // min(-x, -y) = -max(x, y)
        (BinOp::Min, Some(na), Some(nb)) if reuse_a => negate(BinOp::Max, a, ia, [na, nb]),
        (BinOp::Min, Some(na), Some(nb)) if reuse_b => negate(BinOp::Max, b, ib, [na, nb]),
        // max(-x, -y) = -min(x, y)
        (BinOp::Max, Some(na), Some(nb)) if reuse_a => negate(BinOp::Min, a, ia, [na, nb]),
        (BinOp::Max, Some(na), Some(nb)) if reuse_b => negate(BinOp::Min, b, ib, [na, nb]),

        _ => None,
    };

    let mut subtree = if let Some((new_root, r, [a, b])) = indexes {
        *ir = new_root;
        subtree([r, a, b], &mut insts.pool)
    } else {
        [ir, ia, ib]
    };
    rebalance_add_or_sub(&mut subtree, stack, uses);
    rebalance_commutative(&mut insts.vars, uses, stack, subtree);
}

fn rebalance_add_or_sub(
    [inst, inst1, inst2]: &mut [&mut Inst; 3],
    stack: &mut Vec<InstIdx>,
    uses: &[Saturating<u8>],
) -> Option<()> {
    fn add_or_sub(inst: &mut Inst) -> Option<(bool, &mut [InstIdx; 2])> {
        match inst {
            Inst::BinOp { op, args } => match op {
                BinOp::Add => Some((true, args)),
                BinOp::Sub => Some((false, args)),
                _ => None,
            },
            _ => None,
        }
    }

    let (sign1, [w, x]) = add_or_sub(inst1)?;
    let (sign2, [y, z]) = add_or_sub(inst2)?;

    // if both subterms are addition, use the general case
    if sign1 && sign2 {
        return None;
    }

    let (sign, [a, b]) = add_or_sub(inst)?;

    // if either subterm is a subtraction and has other uses, we can't rewrite
    // this subtree
    if !sign1 && uses[usize::from(*a)].0 > 1 {
        return None;
    }
    if !sign2 && uses[usize::from(*b)].0 > 1 {
        return None;
    }

    let (args, args1, args2) = if sign2 {
        debug_assert!(!sign1);
        if sign {
            // (w - x) + (y + z) = (w + (y + z)) - x
            ([*a, *x], Some([*w, *b]), None)
        } else {
            // (w - x) - (y + z) = w - (x + (y + z))
            ([*w, *a], Some([*x, *b]), None)
        }
    } else {
        if !sign {
            // a - (y - z) = a + (z - y)
            swap(y, z);
        }
        if sign1 {
            // (w + x) + (y - z) = ((w + x) + y) - z
            ([*b, *z], None, Some([*a, *y]))
        } else {
            // (w - x) + (y - z) = (w + y) - (x + z)
            ([*a, *b], Some([*w, *y]), Some([*x, *z]))
        }
    };

    // now rewrite subterms, adding them to the stack to revisit if changed
    let op = BinOp::Add;
    debug_assert_eq!(args1.is_none(), sign1);
    if let Some(args) = args1 {
        stack.push(*a);
        **inst1 = Inst::BinOp { op, args };
    }
    debug_assert_eq!(args2.is_none(), sign2);
    if let Some(args) = args2 {
        stack.push(*b);
        **inst2 = Inst::BinOp { op, args };
    }

    // and rewrite the root
    let op = BinOp::Sub;
    **inst = Inst::BinOp { op, args };

    Some(())
}

fn rebalance_commutative(
    vars: &mut [VarSet],
    uses: &[Saturating<u8>],
    stack: &mut Vec<InstIdx>,
    [inst, inst1, inst2]: [&mut Inst; 3],
) {
    // rematch the (possibly rewritten) instruction under the new borrow
    let Inst::BinOp { op, args } = inst else {
        unreachable!()
    };

    if !op.is_commutative() {
        return;
    }
    if let Some(args1) = inst1.is_binop_mut(*op) {
        if let Some(args2) = inst2.is_binop_mut(*op) {
            // don't modify inst1 or inst2 if they have other
            // uses besides in inst, and at this point we can't
            // reassociate without modifying both
            let [idx1, idx2] = args.map(usize::from);
            if uses[idx1].0 < 2 && uses[idx2].0 < 2 {
                rebalance(stack, vars, args, args1, args2);
            }
        } else {
            // inst1 matched but inst2 did not, and they have the
            // same vars, so swap them to establish the invariant
            let [a, b] = args;
            swap(a, b);
        }
    }
}

fn rebalance(
    stack: &mut Vec<InstIdx>,
    vars: &mut [VarSet],
    [a, b]: &mut [InstIdx; 2],
    [a1, b1]: &mut [InstIdx; 2],
    [a2, b2]: &mut [InstIdx; 2],
) {
    // we've matched a happy little balanced tree:
    //   inst(a=inst1(a1, b1), b=inst2(a2, b2))

    let mut varsa1 = vars[usize::from(*a1)];
    let mut varsa2 = vars[usize::from(*a2)];
    let varsb1 = vars[usize::from(*b1)];
    let varsb2 = vars[usize::from(*b2)];

    if varsa1 == varsa2 && (varsa1 < varsb1 || varsa2 < varsb2) {
        // we know:
        //   vars(a1) == vars(a2) -- by this check; call this "little"
        // so if we group a1 with a2 the result will have
        // the same vars as little. but what happens if we
        // group b1 with b2?
        //   little < vars(b1) or little < vars(b2) -- by this check
        //   vars(b1) <= vars(b1) | vars(b2) -- by x <= x | y
        //   vars(b2) <= vars(b1) | vars(b2) -- by x <= x | y
        //   little   <  vars(b1) | vars(b2) -- by x < y <= z implies x < z
        // this satisifies our invariant.
        stack.extend([*a, *b]);
        vars[usize::from(*a)] = varsa1;
        vars[usize::from(*b)] = varsb1 | varsb2;
        swap(b1, a2);
    } else {
        // we want to unbalance this tree into a linked list:
        //   inst(a=a1, b=inst1(a1=a2, b1=inst2(a2=b1, b2)))

        let vars2 = varsb1 | varsb2;
        // inst1 satisfies the invariant, namely
        //   vars(a2) <= vars(b1) | vars(b2)
        debug_assert!(varsa2 <= vars2);
        // because vars(a2) <= vars(b2) (or vars(b1) if we
        // swapped a1/a2 above) and
        //   x <= y implies x <= y | z
        // so we don't need to look at inst1 again here,
        // only inst2
        stack.push(*b);
        vars[usize::from(*a)] = vars2 | varsa2;
        vars[usize::from(*b)] = vars2;

        // swap a1/a2 if necessary so they're in ascending order
        if varsa1 > varsa2 {
            swap(a1, a2);
            swap(&mut varsa1, &mut varsa2);
        }

        let tmp = *b;
        *b = *a;
        *a = *a1;
        *a1 = *a2;
        *a2 = *b1;
        *b1 = tmp;
    }
}
