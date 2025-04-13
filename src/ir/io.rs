use std::collections::{HashMap, hash_map::Entry};
use std::io;
use std::num::ParseFloatError;
use std::str::SplitAsciiWhitespace;
use thiserror::Error;

use super::memoize::Memoized;
use super::{BinOp, Const, Inst, InstIdx, Insts, UnOp, Var, VarSet};

pub fn write(mut f: impl io::Write, insts: impl IntoIterator<Item = Inst>) -> io::Result<()> {
    for (idx, inst) in insts.into_iter().enumerate() {
        write!(f, "v{} ", idx)?;
        match inst {
            Inst::Const { value } => writeln!(f, "const {value}")?,
            Inst::Var { var } => writeln!(f, "var-{}", var.name())?,
            Inst::UnOp { op, arg } => writeln!(f, "{} v{arg}", op.name())?,
            Inst::BinOp { op, args: [a, b] } => writeln!(f, "{} v{a} v{b}", op.name())?,
            Inst::Load => writeln!(f, "load")?,
        };
    }
    Ok(())
}

pub fn write_memoized(mut f: impl io::Write, memoized: &Memoized) -> io::Result<()> {
    writeln!(f, "# consts: {}", memoized.consts.len())?;
    for (idx, value) in memoized.consts.iter().enumerate() {
        writeln!(f, "v{idx} const {value}")?;
    }

    for (idx, func) in memoized.funcs.iter().enumerate() {
        if !func.insts.is_empty() {
            let vars = VarSet((idx + 1) as u8);
            writeln!(f)?;
            writeln!(f, "# func {vars:?}: {} outputs", func.outputs)?;
            for &(reg, vars, loc) in &func.location {
                let mode = if let Inst::Load = func.insts[usize::from(reg)] {
                    "load"
                } else {
                    "store"
                };
                writeln!(f, "# {mode} v{reg} {vars:?}:{loc}")?;
            }
            write(&mut f, func.insts.iter().cloned())?;
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("read failed")]
    IO(#[from] io::Error),
    #[error("invalid constant")]
    InvalidConst(#[from] ParseFloatError),
    #[error("missing token")]
    MissingToken,
    #[error("unexpected token {0:?}")]
    ExtraToken(String),
    #[error("argument uses undefined name {0:?}")]
    UndefinedName(String),
    #[error("instruction redefines existing name {0:?}")]
    RedefinedName(String),
    #[error("unknown instruction {0:?}")]
    UnknownOp(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn read(f: impl io::BufRead) -> Result<Insts> {
    let mut insts = Insts::default();
    let mut names = HashMap::new();

    for line in f.lines() {
        let line = line?;
        if line.starts_with('#') {
            continue;
        }

        let mut tokens = Tokens {
            names: &names,
            tokens: line.split_ascii_whitespace(),
        };

        let out = tokens.next()?;
        let inst = match tokens.next()? {
            "const" => Const::new(tokens.next()?.parse()?).into(),
            "var-x" => Var::X.into(),
            "var-y" => Var::Y.into(),
            "var-z" => Var::Z.into(),

            "neg" => tokens.unop(UnOp::Neg)?,
            "square" => tokens.unop(UnOp::Square)?,
            "sqrt" => tokens.unop(UnOp::Sqrt)?,

            "add" => tokens.binop(BinOp::Add)?,
            "sub" => tokens.binop(BinOp::Sub)?,
            "mul" => tokens.binop(BinOp::Mul)?,
            "min" => tokens.binop(BinOp::Min)?,
            "max" => tokens.binop(BinOp::Max)?,

            op => return Err(Error::UnknownOp(op.to_string())),
        };

        tokens.empty()?;

        match names.entry(out.to_string()) {
            Entry::Vacant(e) => {
                e.insert(insts.push(inst));
            }
            Entry::Occupied(e) => {
                return Err(Error::RedefinedName(e.remove_entry().0));
            }
        }
    }

    Ok(insts)
}

struct Tokens<'a> {
    names: &'a HashMap<String, InstIdx>,
    tokens: SplitAsciiWhitespace<'a>,
}

impl<'a> Tokens<'a> {
    fn next(&mut self) -> Result<&'a str> {
        self.tokens.next().ok_or_else(|| Error::MissingToken)
    }

    fn arg(&mut self) -> Result<InstIdx> {
        let name = self.next()?;
        self.names
            .get(name)
            .ok_or_else(|| Error::UndefinedName(name.to_string()))
            .copied()
    }

    fn unop(&mut self, op: UnOp) -> Result<Inst> {
        Ok(Inst::UnOp {
            op,
            arg: self.arg()?,
        })
    }

    fn binop(&mut self, op: BinOp) -> Result<Inst> {
        Ok(Inst::BinOp {
            op,
            args: [self.arg()?, self.arg()?],
        })
    }

    fn empty(mut self) -> Result<()> {
        if let Some(next) = self.tokens.next() {
            Err(Error::ExtraToken(next.to_string()))
        } else {
            Ok(())
        }
    }
}
