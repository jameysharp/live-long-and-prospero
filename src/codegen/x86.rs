use std::fmt;
use std::io;

use crate::ir::Location;
use crate::ir::{BinOp, UnOp, VarSet};

use super::{AsmInst, MemorySpace, Register};

pub fn write(
    mut f: impl io::Write,
    insts: impl Iterator<Item = AsmInst>,
    stack_slots: Location,
) -> io::Result<()> {
    // requires AVX for v*ss three-operand instructions
    let zero_reg = Xmm(15.try_into().unwrap());

    // prologue
    let frame_size = usize::from(stack_slots) * 4;
    if frame_size > 0 {
        writeln!(f, "sub ${:#x},%rsp", frame_size)?;
    }
    writeln!(f, "xorps {0},{0}", zero_reg)?;

    for inst in insts {
        match inst {
            AsmInst::Const { reg, value } => {
                // Not used for memoized functions
                let scratch_reg = "%eax";
                writeln!(f, "movl ${:#x},{scratch_reg}", value.bits())?;
                writeln!(f, "movd {scratch_reg},{}", Xmm(reg))?;
            }
            AsmInst::Var { reg, var } => {
                // Not used for memoized functions
                let src = Address(VarSet::from(var).into(), 0);
                writeln!(f, "movd {src},{}", Xmm(reg))?;
            }
            AsmInst::UnOp { reg, op, arg } => match op {
                UnOp::Neg => writeln!(f, "vsubss {},{},{}", Xmm(arg), zero_reg, Xmm(reg))?,
                UnOp::Square => writeln!(f, "vmulss {},{},{}", Xmm(arg), Xmm(arg), Xmm(reg))?,
                UnOp::Sqrt => writeln!(f, "sqrtss {},{}", Xmm(arg), Xmm(reg))?,
            },
            AsmInst::BinOp {
                reg,
                op,
                args: [a, b],
            } => {
                let opcode = match op {
                    BinOp::Add => "vaddss",
                    BinOp::Sub => "vsubss",
                    BinOp::Mul => "vmulss",
                    BinOp::Min => "vminss",
                    BinOp::Max => "vmaxss",
                };
                writeln!(f, "{opcode} {},{},{}", Xmm(b), Xmm(a), Xmm(reg))?;
            }
            AsmInst::Load { reg, mem, loc } => {
                writeln!(f, "movd {},{}", Address(mem, loc), Xmm(reg))?;
            }
            AsmInst::Store { reg, mem, loc } => {
                writeln!(f, "movd {},{}", Xmm(reg), Address(mem, loc))?;
            }
        }
    }

    if frame_size > 0 {
        writeln!(f, "add ${:#x},%rsp", frame_size)?;
    }
    writeln!(f, "ret")
}

struct Xmm(Register);

impl fmt::Display for Xmm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "%xmm{}", self.0.idx())
    }
}

struct Address(MemorySpace, Location);

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let memory_space = [
            "(%rsp)",
            "+consts(%rip)",
            "(%rdi)",
            "(%rsi)",
            "(%rdx)",
            "(%rcx)",
            "(%r8)",
            "(%r9)",
            "(%r10)",
        ][self.0.idx()];
        if self.1 > 0 {
            write!(f, "{:#x}", usize::from(self.1) * 4)?;
        }
        write!(f, "{}", memory_space)
    }
}
