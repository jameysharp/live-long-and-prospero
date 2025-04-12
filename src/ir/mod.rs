use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;
use std::hash::Hash;
use std::ops::BitOr;

pub mod io;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct Const(u32);

impl Const {
    pub fn new(v: f32) -> Const {
        assert!(v.is_finite());
        Const(v.to_bits())
    }

    pub fn value(self) -> f32 {
        f32::from_bits(self.0)
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
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Var(u8);

impl Var {
    pub const X: Self = Var(0);
    pub const Y: Self = Var(1);
    pub const Z: Self = Var(2);

    pub fn name(self) -> char {
        (b'x' + self.0).into()
    }
}

pub type InstIdx = u16;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Inst {
    Const { value: Const },
    Var { var: Var },
    UnOp { op: UnOp, arg: InstIdx },
    BinOp { op: BinOp, args: [InstIdx; 2] },
    Load,
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

impl From<Var> for VarSet {
    fn from(value: Var) -> Self {
        VarSet(1 << value.0)
    }
}

impl Iterator for VarSet {
    type Item = Var;

    fn next(&mut self) -> Option<Var> {
        (self.0 != 0).then(|| {
            let var = Var(self.0.trailing_zeros() as u8);
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
                } = self.pool[usize::from(*arg)]
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

        let vars = |idx: InstIdx| self.vars[usize::from(idx)];

        match self.gvn.entry(inst) {
            // If we've already added the same instruction, reuse it.
            Entry::Occupied(e) => *e.get(),

            Entry::Vacant(e) => {
                self.vars.push(match *e.key() {
                    Inst::Const { .. } => VarSet::default(),
                    Inst::Var { var } => var.into(),
                    Inst::UnOp { arg, .. } => vars(arg),
                    Inst::BinOp { args: [a, b], .. } => vars(a) | vars(b),
                    Inst::Load => panic!("use Insts::load to create load instructions"),
                });

                let idx = self.pool.len().try_into().unwrap();
                self.pool.push(e.key().clone());
                *e.insert(idx)
            }
        }
    }

    pub fn load(&mut self, vars: VarSet) -> InstIdx {
        let idx = self.pool.len();
        self.pool.push(Inst::Load);
        self.vars.push(vars);
        idx.try_into().unwrap()
    }

    pub fn vars(&self, idx: InstIdx) -> VarSet {
        self.vars[usize::from(idx)]
    }

    pub fn iter(&self) -> impl Iterator<Item = Inst> {
        self.pool.iter().cloned()
    }
}
