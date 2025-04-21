use std::collections::{HashMap, hash_map::Entry};
use std::io;
use std::num::ParseFloatError;
use thiserror::Error;

use super::memoize::Memoized;
use super::{BinOp, Const, Inst, InstSink, UnOp, Var};

pub fn write(mut f: impl io::Write, insts: impl IntoIterator<Item = Inst>) -> io::Result<()> {
    for (idx, inst) in insts.into_iter().enumerate() {
        write!(f, "v{} ", idx)?;
        match inst {
            Inst::Const { value } => writeln!(f, "const {value}")?,
            Inst::Var { var } => writeln!(f, "var-{}", var.name())?,
            Inst::UnOp { op, arg } => writeln!(f, "{} v{arg}", op.name())?,
            Inst::BinOp { op, args: [a, b] } => writeln!(f, "{} v{a} v{b}", op.name())?,
            Inst::Load { vars, loc } => writeln!(f, "load {vars:?} {loc}")?,
        };
    }
    Ok(())
}

pub fn write_memoized(mut f: impl io::Write, memoized: &Memoized) -> io::Result<()> {
    writeln!(f, "# consts: {}", memoized.consts.len())?;
    for (idx, value) in memoized.consts.iter().enumerate() {
        writeln!(f, "v{idx} const {value}")?;
    }

    for func in memoized.funcs.iter() {
        if !func.insts.is_empty() {
            writeln!(f)?;
            writeln!(f, "# func {:?}: {} outputs", func.vars, func.outputs.len())?;
            write(&mut f, func.insts.iter().cloned())?;
            for (loc, &reg) in func.outputs.iter().enumerate() {
                if let Some(reg) = reg {
                    writeln!(f, "# store v{reg} {:?}:{loc}", func.vars)?;
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("read failed")]
    IO(#[from] io::Error),
    #[error("no input")]
    Empty,
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

pub fn read<S: InstSink>(f: impl io::BufRead, mut sink: S) -> Result<S::Output> {
    let mut names = HashMap::new();
    let mut last = None;

    for line in f.lines() {
        let line = line?;

        let mut tokens = Tokens {
            names: &names,
            tokens: line
                .split_ascii_whitespace()
                .take_while(|token| !token.starts_with('#')),
        };

        let Ok(out) = tokens.next() else { continue };

        let idx = match tokens.next()? {
            "const" => sink.push_const(Const::new(tokens.next()?.parse()?)),
            "var-x" => sink.push_var(Var::X),
            "var-y" => sink.push_var(Var::Y),
            "var-z" => sink.push_var(Var::Z),

            "neg" => tokens.unop(UnOp::Neg, &mut sink)?,
            "square" => tokens.unop(UnOp::Square, &mut sink)?,
            "sqrt" => tokens.unop(UnOp::Sqrt, &mut sink)?,

            "add" => tokens.binop(BinOp::Add, &mut sink)?,
            "sub" => tokens.binop(BinOp::Sub, &mut sink)?,
            "mul" => tokens.binop(BinOp::Mul, &mut sink)?,
            "min" => tokens.binop(BinOp::Min, &mut sink)?,
            "max" => tokens.binop(BinOp::Max, &mut sink)?,

            op => return Err(Error::UnknownOp(op.to_string())),
        };

        tokens.empty()?;

        match names.entry(out.to_string()) {
            Entry::Vacant(e) => {
                e.insert(idx);
            }
            Entry::Occupied(e) => {
                return Err(Error::RedefinedName(e.remove_entry().0));
            }
        }
        last = Some(idx);
    }

    Ok(sink.finish(last.ok_or(Error::Empty)?))
}

struct Tokens<'a, I, S: InstSink> {
    names: &'a HashMap<String, S::Idx>,
    tokens: I,
}

impl<'a, I: Iterator<Item = &'a str>, S: InstSink> Tokens<'a, I, S> {
    fn next(&mut self) -> Result<&'a str> {
        self.tokens.next().ok_or(Error::MissingToken)
    }

    fn arg(&mut self) -> Result<S::Idx> {
        let name = self.next()?;
        self.names
            .get(name)
            .ok_or_else(|| Error::UndefinedName(name.to_string()))
            .copied()
    }

    fn unop(&mut self, op: UnOp, sink: &mut S) -> Result<S::Idx> {
        Ok(sink.push_unop(op, self.arg()?))
    }

    fn binop(&mut self, op: BinOp, sink: &mut S) -> Result<S::Idx> {
        Ok(sink.push_binop(op, [self.arg()?, self.arg()?]))
    }

    fn empty(mut self) -> Result<()> {
        if let Some(next) = self.tokens.next() {
            Err(Error::ExtraToken(next.to_string()))
        } else {
            Ok(())
        }
    }
}
