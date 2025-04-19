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
    let scratch_reg = "rax";

    // prologue
    let frame_size = usize::from(stack_slots) * 4;
    if frame_size > 0 {
        writeln!(f, "sub rsp, {}", frame_size)?;
    }
    writeln!(f, "xorps {0}, {0}", zero_reg)?;

    for inst in insts {
        match inst {
            AsmInst::Const { reg, value } => {
                writeln!(f, "mov {scratch_reg}, {}", value.bits())?;
                writeln!(f, "movd {}, {scratch_reg}", Xmm(reg))?;
            }
            AsmInst::Var { reg, var } => {
                // FIXME stop assuming that var-x is the first value in the x mem-space
                let src = Address(VarSet::from(var).into(), 0);
                writeln!(f, "movd {}, {src}", Xmm(reg))?;
            }
            AsmInst::UnOp { reg, op, arg } => match op {
                UnOp::Neg => writeln!(f, "vsubss {}, {}, {}", Xmm(reg), zero_reg, Xmm(arg))?,
                UnOp::Square => writeln!(f, "vmulss {}, {1}, {1}", Xmm(reg), Xmm(arg))?,
                UnOp::Sqrt => writeln!(f, "sqrtss {}, {}", Xmm(reg), Xmm(arg))?,
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
                writeln!(f, "{opcode} {}, {}, {}", Xmm(reg), Xmm(a), Xmm(b))?;
            }
            AsmInst::Load { reg, mem, loc } => {
                writeln!(f, "movd {}, {}", Xmm(reg), Address(mem, loc))?;
            }
            AsmInst::Store { reg, mem, loc } => {
                writeln!(f, "movd {}, {}", Address(mem, loc), Xmm(reg))?;
            }
        }
    }

    if frame_size > 0 {
        writeln!(f, "add rsp, {}", frame_size)?;
    }
    writeln!(f, "ret")
}

struct Xmm(Register);

impl fmt::Display for Xmm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "xmm{}", self.0.idx())
    }
}

struct Address(MemorySpace, Location);

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let memory_spaces = ["rsp", "rdi", "rsi", "rdx", "rcx", "r8", "r9", "r10", "r11"];
        write!(f, "[{}", memory_spaces[self.0.idx()])?;
        if self.1 > 0 {
            write!(f, "+{}", usize::from(self.1) * 4)?;
        }
        write!(f, "]")
    }
}
