use std::collections::HashMap;

use super::{BinOp, Const, InstSink, Location, UnOp, Var, VarSet};

pub struct Simplify<S: InstSink> {
    base: S,
    gvn: HashMap<Key<S::Idx>, S::Idx>,
}

impl<S: InstSink> Simplify<S> {
    pub fn new(base: S) -> Self {
        let gvn = HashMap::new();
        Self { base, gvn }
    }

    fn gvn_binop(&mut self, op: BinOp, mut args: [S::Idx; 2]) -> Idx<S::Idx> {
        match op {
            // Sort arguments to commutative binary operators so GVN is more effective.
            BinOp::Add | BinOp::Mul | BinOp::Min | BinOp::Max => args.sort_unstable(),

            // Subtraction is not commutative, but if we previously subtracted the
            // arguments in the opposite order then we can just negate that previous
            // result.
            BinOp::Sub => {
                let [a, b] = args;
                let reversed = Key::BinOp(op, [b, a]);
                if let Some(&idx) = self.gvn.get(&reversed) {
                    return Idx::Neg(idx);
                }
            }
        }

        self.gvn
            .entry(Key::BinOp(op, args))
            .or_insert_with(|| self.base.push_binop(op, args))
            .into()
    }

    fn gvn_unop(&mut self, op: UnOp, arg: S::Idx) -> S::Idx {
        *self
            .gvn
            .entry(Key::UnOp(op, arg))
            .or_insert_with(|| self.base.push_unop(op, arg))
    }

    fn force_neg(&mut self, arg: Idx<S::Idx>) -> S::Idx {
        match arg {
            Idx::Neg(arg) => self.gvn_unop(UnOp::Neg, arg),
            Idx::Pos(arg) => arg,
        }
    }
}

#[derive(Debug, Eq, Hash, PartialEq)]
enum Key<I> {
    Const(Const),
    Var(Var),
    UnOp(UnOp, I),
    BinOp(BinOp, [I; 2]),
    Load(VarSet, Location),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Idx<I> {
    Pos(I),
    Neg(I),
}

impl<I> Idx<I> {
    fn negate(self) -> Self {
        match self {
            Idx::Pos(x) => Idx::Neg(x),
            Idx::Neg(x) => Idx::Pos(x),
        }
    }
}

impl<I: Copy> From<&mut I> for Idx<I> {
    fn from(value: &mut I) -> Self {
        Idx::Pos(*value)
    }
}

impl<S: InstSink> InstSink for Simplify<S> {
    type Idx = Idx<S::Idx>;
    type Output = S::Output;

    fn push_const(&mut self, value: Const) -> Self::Idx {
        self.gvn
            .entry(Key::Const(value))
            .or_insert_with(|| self.base.push_const(value))
            .into()
    }

    fn push_var(&mut self, var: Var) -> Self::Idx {
        self.gvn
            .entry(Key::Var(var))
            .or_insert_with(|| self.base.push_var(var))
            .into()
    }

    fn push_unop(&mut self, op: UnOp, arg: Self::Idx) -> Self::Idx {
        let arg = match op {
            // Delay creating Neg instructions in case we can simplify them away.
            UnOp::Neg => return arg.negate(),

            // Squaring -x is the same as squaring x, so ignore negation.
            UnOp::Square => match arg {
                Idx::Pos(x) | Idx::Neg(x) => x,
            },

            // For other operators, emit a Neg first if necessary.
            _ => self.force_neg(arg),
        };
        Idx::Pos(self.gvn_unop(op, arg))
    }

    fn push_binop(&mut self, op: BinOp, args: [Self::Idx; 2]) -> Self::Idx {
        let (op, args, negated) = match (op, args) {
            (op, [Idx::Pos(a), Idx::Pos(b)]) => (op, [a, b], false),

            // (-x) + (-y) = -(x + y)
            (BinOp::Add, [Idx::Neg(a), Idx::Neg(b)]) => (BinOp::Add, [a, b], true),
            // x + (-y) = x - y
            (BinOp::Add, [Idx::Pos(a), Idx::Neg(b)]) => (BinOp::Sub, [a, b], false),
            // (-x) + y = y - x
            (BinOp::Add, [Idx::Neg(a), Idx::Pos(b)]) => (BinOp::Sub, [b, a], false),

            // (-x) - (-y) = y - x
            (BinOp::Sub, [Idx::Neg(a), Idx::Neg(b)]) => (BinOp::Sub, [b, a], false),
            // x - (-y) = x + y
            (BinOp::Sub, [Idx::Pos(a), Idx::Neg(b)]) => (BinOp::Add, [a, b], false),
            // (-x) - y = -(x + y)
            (BinOp::Sub, [Idx::Neg(a), Idx::Pos(b)]) => (BinOp::Add, [a, b], true),

            // (-x) * (-y) = x * y
            (BinOp::Mul, [Idx::Neg(a), Idx::Neg(b)]) => (BinOp::Mul, [a, b], false),
            // x * (-y) = -(x * y)
            (BinOp::Mul, [Idx::Pos(a), Idx::Neg(b)]) => (BinOp::Mul, [a, b], true),
            // (-x) * y = -(x * y)
            (BinOp::Mul, [Idx::Neg(a), Idx::Pos(b)]) => (BinOp::Mul, [a, b], true),

            // min(-x, -y) = -max(x, y)
            (BinOp::Min, [Idx::Neg(a), Idx::Neg(b)]) => (BinOp::Max, [a, b], true),
            // max(-x, -y) = -min(x, y)
            (BinOp::Max, [Idx::Neg(a), Idx::Neg(b)]) => (BinOp::Min, [a, b], true),

            (op, [Idx::Pos(a), Idx::Neg(b)]) => (op, [a, self.gvn_unop(UnOp::Neg, b)], false),
            (op, [Idx::Neg(a), Idx::Pos(b)]) => (op, [self.gvn_unop(UnOp::Neg, a), b], false),
        };

        let idx = self.gvn_binop(op, args);
        if negated { idx.negate() } else { idx }
    }

    fn push_load(&mut self, vars: VarSet, loc: Location) -> Self::Idx {
        self.gvn
            .entry(Key::Load(vars, loc))
            .or_insert_with(|| self.base.push_load(vars, loc))
            .into()
    }

    fn finish(mut self, last: Self::Idx) -> Self::Output {
        let last = self.force_neg(last);
        self.base.finish(last)
    }
}
