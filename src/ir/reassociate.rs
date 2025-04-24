use std::mem::swap;
use std::num::Saturating;

use super::{BinOp, Inst, InstSink, UnOp, VarSet};

pub fn reassociate<S: InstSink>(insts: &[Inst], mut sink: S) -> S::Output {
    let uses = count_uses(insts);
    let mut data: Vec<InstData<S::Idx>> = Vec::with_capacity(insts.len());

    for (inst, uses) in insts.iter().zip(uses) {
        let mut new = match *inst {
            Inst::Const { value } => InstData::new(VarSet::default(), sink.push_const(value)),
            Inst::Var { var } => InstData::new(var.into(), sink.push_var(var)),
            Inst::Load { vars, loc } => InstData::new(vars, sink.push_load(vars, loc)),
            Inst::UnOp { op, arg } => {
                let mut arg = data[arg.idx()].clone();
                if op == UnOp::Neg {
                    arg.negate();
                    arg
                } else {
                    let (vars, idx) = arg.flush_neg(&mut sink);
                    InstData::new(vars, sink.push_unop(op, idx))
                }
            }
            Inst::BinOp { mut op, args } => {
                let [mut a, mut b] = args.map(|arg| data[arg.idx()].clone());
                if op == BinOp::Sub {
                    op = BinOp::Add;
                    b.negate();
                }
                if a.op != Some(op) {
                    a.flush(&mut sink);
                }
                if b.op != Some(op) {
                    b.flush(&mut sink);
                }
                for (subtree_a, subtree_b) in a.subtrees.iter_mut().zip(&b.subtrees) {
                    subtree_a.merge(subtree_b, op, &mut sink);
                }
                a.op = Some(op);
                a
            }
        };
        if uses.0 > 1 {
            new.flush(&mut sink);
        }
        data.push(new);
    }

    let last = data.pop().unwrap();
    let (_vars, last) = last.flush_neg(&mut sink);
    sink.finish(last)
}

#[derive(Clone, Debug)]
struct InstData<I> {
    op: Option<BinOp>,
    subtrees: [Subtree<I>; VarSet::ALL.idx() + 1],
}

impl<I: Copy> InstData<I> {
    fn new(vars: VarSet, idx: I) -> Self {
        let mut result = InstData {
            op: None,
            subtrees: Default::default(),
        };
        result.subtrees[vars.idx()].pos = Some(idx);
        result
    }

    fn flush(&mut self, sink: &mut impl InstSink<Idx = I>) {
        if let Some(op) = self.op {
            let mut result = Subtree::default();
            let mut result_vars = VarSet::default();
            // The results of this pass are very sensitive to the details of
            // this loop. There are three key choices here:
            // - Largest VarSet to smallest (with .rev()) or vice versa?
            // - Flush each subtree before merging, or not?
            // - Flush immediately after merging, or once at the end?
            // I don't have strong justification for any of these choices,
            // but empirically, this combination is better than all the
            // alternatives, both at minimizing the number of outputs needed
            // from each memoized function, and at moving more instructions
            // out of the final function that runs for each pixel and into the
            // memoized functions that run much less often.
            for (idx, subtree) in self.subtrees.iter_mut().enumerate().rev() {
                if !subtree.is_empty() {
                    subtree.flush(op, sink);
                    result.merge(subtree, op, sink);
                    result.flush(op, sink);
                    result_vars = result_vars | VarSet(idx.try_into().unwrap());
                    *subtree = Subtree::default();
                }
            }
            debug_assert!(!result.is_empty());
            self.subtrees[result_vars.idx()] = result;
            self.op = None;
        }
    }

    fn flush_neg(mut self, sink: &mut impl InstSink<Idx = I>) -> (VarSet, I) {
        self.flush(sink);
        let mut it = self.subtrees.iter().enumerate();
        let (vars, subtree) = it.find(|(_vars, subtree)| !subtree.is_empty()).unwrap();
        debug_assert_ne!(subtree.pos.is_some(), subtree.neg.is_some());
        debug_assert!(it.all(|(_vars, subtree)| subtree.is_empty()));

        let idx = match (subtree.pos, subtree.neg) {
            (Some(pos), None) => pos,
            (None, Some(neg)) => sink.push_unop(UnOp::Neg, neg),
            _ => unreachable!(),
        };
        (VarSet(vars.try_into().unwrap()), idx)
    }

    fn negate(&mut self) {
        for vars in self.subtrees.iter_mut() {
            vars.negate();
        }

        match self.op {
            Some(BinOp::Min) => self.op = Some(BinOp::Max),
            Some(BinOp::Max) => self.op = Some(BinOp::Min),
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
struct Subtree<I> {
    pos: Option<I>,
    neg: Option<I>,
}

impl<I> Default for Subtree<I> {
    fn default() -> Self {
        Subtree {
            pos: None,
            neg: None,
        }
    }
}

impl<I: Copy> Subtree<I> {
    fn is_empty(&self) -> bool {
        self.pos.is_none() && self.neg.is_none()
    }

    fn flush(&mut self, op: BinOp, sink: &mut impl InstSink<Idx = I>) {
        if let (Some(pos), Some(neg)) = (self.pos, self.neg) {
            let pos = match op {
                BinOp::Sub | BinOp::Mul => unreachable!(),
                BinOp::Add => sink.push_binop(BinOp::Sub, [pos, neg]),
                BinOp::Min | BinOp::Max => {
                    let neg = sink.push_unop(UnOp::Neg, neg);
                    sink.push_binop(op, [pos, neg])
                }
            };
            self.pos = Some(pos);
            self.neg = None;
        }
    }

    fn negate(&mut self) {
        swap(&mut self.pos, &mut self.neg);
    }

    fn merge(&mut self, other: &Self, op: BinOp, sink: &mut impl InstSink<Idx = I>) {
        match op {
            BinOp::Sub => unreachable!(),
            BinOp::Mul => {
                debug_assert!(self.pos.is_none() || self.neg.is_none());
                debug_assert!(other.pos.is_none() || other.neg.is_none());
                let neg = self.neg.is_some() ^ other.neg.is_some();
                *self = Subtree {
                    pos: self.pos.or(self.neg),
                    neg: None,
                };
                merge(&mut self.pos, other.pos.or(other.neg), BinOp::Mul, sink);
                if neg {
                    self.negate();
                }
            }
            BinOp::Add => {
                merge(&mut self.pos, other.pos, BinOp::Add, sink);
                merge(&mut self.neg, other.neg, BinOp::Add, sink);
            }
            BinOp::Min => {
                merge(&mut self.pos, other.pos, BinOp::Min, sink);
                merge(&mut self.neg, other.neg, BinOp::Max, sink);
            }
            BinOp::Max => {
                merge(&mut self.pos, other.pos, BinOp::Max, sink);
                merge(&mut self.neg, other.neg, BinOp::Min, sink);
            }
        }
    }
}

fn merge<S: InstSink>(this: &mut Option<S::Idx>, other: Option<S::Idx>, op: BinOp, sink: &mut S) {
    if let Some(b) = other {
        if let Some(a) = *this {
            *this = Some(sink.push_binop(op, [a, b]));
        } else {
            *this = Some(b);
        }
    }
}

fn count_uses(insts: &[Inst]) -> Vec<Saturating<u8>> {
    let mut uses = vec![Saturating(0u8); insts.len()];
    if let Some(last) = uses.last_mut() {
        last.0 = 1;
    }
    for (idx, inst) in insts.iter().enumerate().rev() {
        if uses[idx].0 > 0 {
            for &arg in inst.args() {
                uses[arg.idx()] += 1;
            }
        }
    }
    uses
}
