use std::fmt;
use std::io;

use crate::ir::memoize::MemoizedFunc;
use crate::ir::{BinOp, Location, UnOp, VarSet};

use super::{AsmInst, MemorySpace, Register};

pub const STRIDE: u8 = 4;

pub fn write(
    mut f: impl io::Write,
    func: &MemoizedFunc,
    vectors: impl IntoIterator<Item = VarSet>,
) -> io::Result<()> {
    // requires AVX for v*ps three-operand instructions
    let (insts, stack_slots) = super::regalloc::alloc(&func.insts, func.vars, &func.outputs);

    let vectors = vectors.into_iter().fold(0, |set, vars| {
        set | (1 << MemorySpace::from(vars).idx()) | 1
    });
    let stride = if vectors != 0 { STRIDE } else { 1 };

    let zero_reg = Xmm(15.try_into().unwrap());

    // prologue
    let frame_size = usize::from(stack_slots) * usize::from(stride) * 4;
    if frame_size > 0 {
        writeln!(f, "pushq %rbp")?;
        writeln!(f, "movq %rsp,%rbp")?;
        writeln!(f, "sub ${:#x},%rsp", frame_size)?;
    }
    writeln!(f, "xorps {0},{0}", zero_reg)?;

    for inst in insts.into_iter().rev() {
        match inst {
            AsmInst::Const { .. } => todo!(),
            AsmInst::Var { .. } => todo!(),
            AsmInst::UnOp { reg, op, arg } => match op {
                UnOp::Neg => writeln!(f, "vsubps {},{},{}", Xmm(arg), zero_reg, Xmm(reg))?,
                UnOp::Square => writeln!(f, "vmulps {},{},{}", Xmm(arg), Xmm(arg), Xmm(reg))?,
                UnOp::Sqrt => writeln!(f, "sqrtps {},{}", Xmm(arg), Xmm(reg))?,
            },
            AsmInst::BinOp {
                reg,
                op,
                args: [a, b],
            } => {
                let opcode = match op {
                    BinOp::Add => "vaddps",
                    BinOp::Sub => "vsubps",
                    BinOp::Mul => "vmulps",
                    BinOp::Min => "vminps",
                    BinOp::Max => "vmaxps",
                };
                writeln!(f, "{opcode} {},{},{}", Xmm(b), Xmm(a), Xmm(reg))?;
            }
            AsmInst::Load { reg, mem, loc } => {
                let opcode = if vectors & (1 << mem.idx()) != 0 {
                    "movaps"
                } else {
                    "vbroadcastss"
                };
                let stride = if mem == VarSet::default().into() {
                    // constants are always scalars
                    1
                } else {
                    stride
                };
                writeln!(f, "{opcode} {},{}", Address(mem, loc, stride), Xmm(reg))?;
            }
            AsmInst::Store { reg, mem, loc } => {
                let opcode = if vectors & (1 << mem.idx()) != 0 {
                    "movaps"
                } else {
                    "movd"
                };
                writeln!(f, "{opcode} {},{}", Xmm(reg), Address(mem, loc, stride))?;
            }
        }
    }

    if frame_size > 0 {
        writeln!(f, "movq %rbp,%rsp")?;
        writeln!(f, "pop %rbp")?;
    }
    writeln!(f, "ret")
}

struct Xmm(Register);

impl fmt::Display for Xmm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "%xmm{}", self.0.idx())
    }
}

struct Address(MemorySpace, Location, u8);

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
            write!(f, "{:#x}", usize::from(self.1) * usize::from(self.2) * 4)?;
        }
        write!(f, "{}", memory_space)
    }
}
