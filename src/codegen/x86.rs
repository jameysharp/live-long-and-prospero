use std::fmt;
use std::io;

use crate::codegen::regalloc::{Allocation, Registers};
use crate::ir::memoize::{Memoized, MemoizedFunc};
use crate::ir::{BinOp, Inst, Location, UnOp, Var, VarSet};

use super::regalloc::Target;
use super::{MemorySpace, Register};

const STRIDE: u8 = 4;

pub fn write(mut out: impl io::Write, memoized: &Memoized) -> io::Result<()> {
    writeln!(
        out,
        "# compile with: gcc -Wall -g -O2 -o <output> examples/x86-harness.c <output>.s"
    )?;
    writeln!(out, ".section .rodata")?;
    writeln!(out, "consts:")?;
    writeln!(out, ".align 4")?;
    for (idx, value) in memoized.consts.iter().enumerate() {
        writeln!(out, ".L{idx}: .long {:#08x}", value.bits())?;
    }
    writeln!(out, ".globl stride")?;
    writeln!(out, "stride: .short {}", STRIDE)?;
    Ok(for func in memoized.funcs.iter() {
        if !func.insts.is_empty() {
            writeln!(out)?;
            writeln!(out, ".section .rodata")?;
            writeln!(out, ".globl {:?}_size", func.vars)?;
            writeln!(out, "{:?}_size:", func.vars)?;
            writeln!(out, ".short {}", func.outputs.len())?;

            writeln!(out)?;
            writeln!(out, ".text")?;
            writeln!(out, ".p2align 4")?;
            writeln!(out, ".globl {:?}", func.vars)?;
            writeln!(out, "{:?}:", func.vars)?;
            write_func(&mut out, func, [func.vars, Var::X.into()])?;
        }
    })
}

fn write_func(
    mut f: impl io::Write,
    func: &MemoizedFunc,
    vectors: impl IntoIterator<Item = VarSet>,
) -> io::Result<()> {
    // requires AVX for v*ps three-operand instructions

    let mut allocs: Vec<Allocation> = func
        .insts
        .iter()
        .map(|inst| {
            let mut alloc = Allocation::default();
            if let Inst::Load { vars, loc } = *inst {
                alloc.initial_location(vars.into(), loc);
            }
            alloc
        })
        .collect();

    for (loc, &idx) in func.outputs.iter().enumerate() {
        if let Some(idx) = idx {
            allocs[idx.idx()].initial_location(func.vars.into(), loc.try_into().unwrap());
        }
    }

    let mut regs = Registers::new(allocs, 15, Vec::new());

    for (idx, inst) in func.insts.iter().enumerate().rev() {
        let idx = idx.try_into().unwrap();
        match *inst {
            Inst::Const { .. } | Inst::Var { .. } => {
                unimplemented!("{inst:?} not allowed in memoized functions")
            }
            Inst::UnOp { op, arg } => {
                let reg = regs.get_output_reg(idx);
                let arg = regs.get_reg(arg);
                regs.target.push(AsmInst::UnOp { reg, op, arg });
            }
            Inst::BinOp { op, args } => {
                let reg = regs.get_output_reg(idx);
                let args = args.map(|arg| regs.get_reg(arg));
                regs.target.push(AsmInst::BinOp { reg, op, args });
            }
            Inst::Load { vars, loc } => regs.emit_load(idx, vars, loc),
        }
    }

    let (insts, stack_slots) = regs.finish();

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AsmInst {
    UnOp {
        reg: Register,
        op: UnOp,
        arg: Register,
    },
    BinOp {
        reg: Register,
        op: BinOp,
        args: [Register; 2],
    },
    Load {
        reg: Register,
        mem: MemorySpace,
        loc: Location,
    },
    Store {
        reg: Register,
        mem: MemorySpace,
        loc: Location,
    },
}

impl Target for Vec<AsmInst> {
    fn emit_load(&mut self, reg: Register, mem: MemorySpace, loc: Location) {
        self.push(AsmInst::Load { reg, mem, loc });
    }

    fn emit_store(&mut self, reg: Register, mem: MemorySpace, loc: Location) {
        self.push(AsmInst::Store { reg, mem, loc });
    }
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
