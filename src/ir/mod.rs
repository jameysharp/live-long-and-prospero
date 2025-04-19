use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;
use std::hash::Hash;
use std::num::{NonZeroU16, TryFromIntError};
use std::ops::{BitOr, Deref};

pub mod interp;
pub mod io;
pub mod memoize;
pub mod reassociate;
pub mod reorder;

#[derive(Clone, Copy, Default, Eq, Hash, PartialEq)]
pub struct Const(u32);

impl Const {
    pub fn new(v: f32) -> Const {
        assert!(v.is_finite());
        Const(v.to_bits())
    }

    pub fn value(self) -> f32 {
        f32::from_bits(self.0)
    }

    pub fn bits(self) -> u32 {
        self.0
    }
}

impl fmt::Display for Const {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.value())
    }
}

impl fmt::Debug for Const {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.value())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum UnOp {
    Neg,
    Square,
    Sqrt,
}

impl UnOp {
    pub fn name(self) -> &'static str {
        match self {
            UnOp::Neg => "neg",
            UnOp::Square => "square",
            UnOp::Sqrt => "sqrt",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Min,
    Max,
}

impl BinOp {
    pub fn name(self) -> &'static str {
        match self {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::Min => "min",
            BinOp::Max => "max",
        }
    }

    pub fn is_commutative(self) -> bool {
        match self {
            BinOp::Add => true,
            BinOp::Sub => false,
            BinOp::Mul => true,
            BinOp::Min => true,
            BinOp::Max => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Var {
    X,
    Y,
    Z,
}

impl Var {
    pub fn name(self) -> char {
        (b'x' + self as u8).into()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InstIdx(NonZeroU16);

impl InstIdx {
    pub const fn idx(self) -> usize {
        self.0.get() as usize - 1
    }
}

impl TryFrom<usize> for InstIdx {
    type Error = TryFromIntError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u16::try_from(value.wrapping_add(1))
            .and_then(NonZeroU16::try_from)
            .map(InstIdx)
    }
}

impl fmt::Display for InstIdx {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.idx())
    }
}

pub type Location = u16;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Inst {
    Const { value: Const },
    Var { var: Var },
    UnOp { op: UnOp, arg: InstIdx },
    BinOp { op: BinOp, args: [InstIdx; 2] },
    Load { vars: VarSet, loc: Location },
}

impl Inst {
    pub fn args(&self) -> &[InstIdx] {
        match self {
            Inst::Const { .. } | Inst::Var { .. } | Inst::Load { .. } => &[],
            Inst::UnOp { arg, .. } => std::slice::from_ref(arg),
            Inst::BinOp { args, .. } => args,
        }
    }

    pub fn args_mut(&mut self) -> &mut [InstIdx] {
        match self {
            Inst::Const { .. } | Inst::Var { .. } | Inst::Load { .. } => &mut [],
            Inst::UnOp { arg, .. } => std::slice::from_mut(arg),
            Inst::BinOp { args, .. } => args,
        }
    }

    pub fn is_unop(&self, expected: UnOp) -> Option<InstIdx> {
        if let &Inst::UnOp { op, arg } = self {
            if op == expected {
                return Some(arg);
            }
        }
        None
    }

    pub fn is_binop_mut(&mut self, expected: BinOp) -> Option<&mut [InstIdx; 2]> {
        if let Inst::BinOp { op, args } = self {
            if *op == expected {
                return Some(args);
            }
        }
        None
    }
}

impl From<Const> for Inst {
    fn from(value: Const) -> Self {
        Inst::Const { value }
    }
}

impl From<Var> for Inst {
    fn from(var: Var) -> Self {
        Inst::Var { var }
    }
}

#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VarSet(u8);

impl VarSet {
    pub const ALL: VarSet = VarSet(7);

    pub const fn idx(self) -> usize {
        self.0 as usize
    }
}

impl From<Var> for VarSet {
    fn from(value: Var) -> Self {
        VarSet(1 << value as u8)
    }
}

impl Iterator for VarSet {
    type Item = Var;

    fn next(&mut self) -> Option<Var> {
        (self.0 != 0).then(|| {
            let var = match self.0.trailing_zeros() {
                0 => Var::X,
                1 => Var::Y,
                2 => Var::Z,
                _ => unreachable!(),
            };
            self.0 &= !VarSet::from(var).0;
            var
        })
    }
}

impl fmt::Debug for VarSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut empty = true;
        for var in *self {
            write!(f, "{}", var.name())?;
            empty = false;
        }
        if empty {
            write!(f, "const")?;
        }
        Ok(())
    }
}

impl BitOr for VarSet {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        VarSet(self.0 | rhs.0)
    }
}

#[derive(Default)]
pub struct Insts {
    pool: Vec<Inst>,
    vars: Vec<VarSet>,
    gvn: HashMap<Inst, InstIdx>,
}

impl Insts {
    pub fn push(&mut self, mut inst: Inst) -> InstIdx {
        match &mut inst {
            Inst::UnOp {
                op: UnOp::Square,
                arg,
            } => {
                // Squaring -x is the same as squaring x.
                if let Inst::UnOp {
                    op: UnOp::Neg,
                    arg: pos,
                } = self.pool[arg.idx()]
                {
                    *arg = pos;
                }
            }

            Inst::BinOp { op, args } => {
                match op {
                    // Sort arguments to commutative binary operators so GVN is more effective.
                    BinOp::Add | BinOp::Mul | BinOp::Min | BinOp::Max => args.sort_unstable(),

                    // Subtraction is not commutative, but if we previously subtracted the
                    // arguments in the opposite order then we can just negate that previous
                    // result.
                    BinOp::Sub => {
                        let [a, b] = *args;
                        let reversed = Inst::BinOp {
                            op: BinOp::Sub,
                            args: [b, a],
                        };
                        if let Some(&idx) = self.gvn.get(&reversed) {
                            inst = Inst::UnOp {
                                op: UnOp::Neg,
                                arg: idx,
                            };
                        }
                    }
                }
            }
            _ => (),
        }

        let vars = |idx: InstIdx| self.vars[idx.idx()];

        match self.gvn.entry(inst) {
            // If we've already added the same instruction, reuse it.
            Entry::Occupied(e) => *e.get(),

            Entry::Vacant(e) => {
                self.vars.push(match *e.key() {
                    Inst::Const { .. } => VarSet::default(),
                    Inst::Var { var } => var.into(),
                    Inst::UnOp { arg, .. } => vars(arg),
                    Inst::BinOp { args: [a, b], .. } => vars(a) | vars(b),
                    Inst::Load { vars, .. } => vars,
                });

                let idx = self.pool.len().try_into().unwrap();
                self.pool.push(e.key().clone());
                *e.insert(idx)
            }
        }
    }

    pub fn vars(&self, idx: InstIdx) -> VarSet {
        self.vars[idx.idx()]
    }
}

impl Deref for Insts {
    type Target = [Inst];

    fn deref(&self) -> &Self::Target {
        &self.pool
    }
}
