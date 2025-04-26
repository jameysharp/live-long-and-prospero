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
    writeln!(out, ".p2align 4")?;
    for (idx, value) in memoized.consts.iter().enumerate() {
        write!(out, ".L{idx}:")?;
        for _ in 0..STRIDE {
            writeln!(out, " .long {:#08x}", value.bits())?;
        }
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

fn emit(
    func: &MemoizedFunc,
    vectors: impl IntoIterator<Item = VarSet>,
) -> (Xmm, X86Target, Location) {
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

    let zero_reg = Xmm(15.try_into().unwrap());
    let mut regs = Registers::new(allocs, 15, X86Target::new(vectors));

    for (idx, inst) in func.insts.iter().enumerate().rev() {
        let idx = idx.try_into().unwrap();
        match *inst {
            Inst::Const { .. } | Inst::Var { .. } => {
                unimplemented!("{inst:?} not allowed in memoized functions")
            }
            Inst::UnOp { op, arg } => {
                let dst = regs.get_output_reg(idx).into();
                let arg: Xmm = regs.get_reg(arg).into();
                let inst = match op {
                    UnOp::Neg => X86Inst::XmmRmR {
                        op: BinOp::Sub,
                        src1: zero_reg,
                        src2: arg.into(),
                        dst,
                    },
                    UnOp::Square => X86Inst::XmmRmR {
                        op: BinOp::Mul,
                        src1: arg,
                        src2: arg.into(),
                        dst,
                    },
                    UnOp::Sqrt => X86Inst::XmmUnaryRmRVex {
                        op: XmmUnaryRmRVexOpcode::Vsqrtps,
                        src: arg.into(),
                        dst,
                    },
                };
                regs.target.insts.push(inst);
            }
            Inst::BinOp { op, args } => {
                let dst = regs.get_output_reg(idx).into();
                let [src1, src2] = args.map(|arg| regs.get_reg(arg).into());
                regs.target.insts.push(X86Inst::XmmRmR {
                    op,
                    src1,
                    src2: src2.into(),
                    dst,
                });
            }
            Inst::Load { vars, loc } => regs.emit_load(idx, vars.into(), loc),
        }
    }

    let (target, stack_slots) = regs.finish();
    (zero_reg, target, stack_slots)
}

fn write_func(
    mut f: impl io::Write,
    func: &MemoizedFunc,
    vectors: impl IntoIterator<Item = VarSet>,
) -> io::Result<()> {
    let (zero_reg, target, stack_slots) = emit(func, vectors);

    // prologue
    let frame_size = usize::from(stack_slots) * usize::from(target.stride) * 4;
    if frame_size > 0 {
        writeln!(f, "pushq %rbp")?;
        writeln!(f, "movq %rsp,%rbp")?;
        writeln!(f, "sub ${:#x},%rsp", frame_size)?;
    }
    writeln!(f, "xorps {0},{0}", zero_reg)?;

    for inst in target.insts.into_iter().rev() {
        writeln!(f, "{inst}")?;
    }

    if frame_size > 0 {
        writeln!(f, "movq %rbp,%rsp")?;
        writeln!(f, "pop %rbp")?;
    }
    writeln!(f, "ret")
}

struct X86Target {
    vectors: u16,
    stride: u8,
    insts: Vec<X86Inst>,
}

impl X86Target {
    fn new(vectors: impl IntoIterator<Item = VarSet>) -> X86Target {
        let vectors = vectors.into_iter().fold(0, |set, vars| {
            set | (1 << MemorySpace::from(vars).idx()) | 0b11
        });
        X86Target {
            vectors,
            stride: if vectors != 0 { STRIDE } else { 1 },
            insts: Vec::new(),
        }
    }
}

impl Target for X86Target {
    fn emit_load(&mut self, reg: Register, mem: MemorySpace, loc: Location) {
        let op = if self.vectors & (1 << mem.idx()) != 0 {
            XmmUnaryRmRVexOpcode::Vmovaps
        } else {
            XmmUnaryRmRVexOpcode::Vbroadcastss
        };
        let dst = reg.into();
        let src = Address(mem, loc, self.stride).into();
        self.insts.push(X86Inst::XmmUnaryRmRVex { op, src, dst });
    }

    fn emit_store(&mut self, reg: Register, mem: MemorySpace, loc: Location) {
        let op = if self.vectors & (1 << mem.idx()) != 0 {
            XmmMovRMVexOpcode::Vmovaps
        } else {
            XmmMovRMVexOpcode::Vmovd
        };
        let src = reg.into();
        let dst = Address(mem, loc, self.stride).into();
        self.insts.push(X86Inst::XmmMovRMVex { op, src, dst });
    }
}

#[derive(Debug)]
enum X86Inst {
    XmmRmR {
        op: BinOp,
        src1: Xmm,
        src2: XmmMem,
        dst: Xmm,
    },
    XmmUnaryRmRVex {
        op: XmmUnaryRmRVexOpcode,
        src: XmmMem,
        dst: Xmm,
    },
    XmmMovRMVex {
        op: XmmMovRMVexOpcode,
        src: Xmm,
        dst: XmmMem,
    },
}

impl fmt::Display for X86Inst {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            X86Inst::XmmRmR {
                op,
                src1,
                src2,
                dst,
            } => {
                let opcode = match op {
                    BinOp::Add => "vaddps",
                    BinOp::Sub => "vsubps",
                    BinOp::Mul => "vmulps",
                    BinOp::Min => "vminps",
                    BinOp::Max => "vmaxps",
                };
                write!(f, "{opcode} {src2},{src1},{dst}")
            }
            X86Inst::XmmUnaryRmRVex { op, src, dst } => {
                let opcode = match op {
                    XmmUnaryRmRVexOpcode::Vmovaps => "vmovaps",
                    XmmUnaryRmRVexOpcode::Vbroadcastss => "vbroadcastss",
                    XmmUnaryRmRVexOpcode::Vsqrtps => "vsqrtps",
                };
                write!(f, "{opcode} {src},{dst}")
            }
            X86Inst::XmmMovRMVex { op, src, dst } => {
                let opcode = match op {
                    XmmMovRMVexOpcode::Vmovaps => "vmovaps",
                    XmmMovRMVexOpcode::Vmovd => "vmovd",
                };
                write!(f, "{opcode} {src},{dst}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum XmmUnaryRmRVexOpcode {
    Vbroadcastss,
    Vmovaps,
    Vsqrtps,
}

#[derive(Clone, Copy, Debug)]
enum XmmMovRMVexOpcode {
    Vmovaps,
    Vmovd,
}

#[derive(Clone, Copy, Debug)]
enum XmmMem {
    Xmm(Xmm),
    Mem(Address),
}

impl fmt::Display for XmmMem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            XmmMem::Xmm(xmm) => xmm.fmt(f),
            XmmMem::Mem(address) => address.fmt(f),
        }
    }
}

impl From<Xmm> for XmmMem {
    fn from(value: Xmm) -> Self {
        XmmMem::Xmm(value)
    }
}

impl From<Address> for XmmMem {
    fn from(value: Address) -> Self {
        XmmMem::Mem(value)
    }
}

#[derive(Clone, Copy, Debug)]
struct Xmm(Register);

impl fmt::Display for Xmm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "%xmm{}", self.0.idx())
    }
}

impl From<Register> for Xmm {
    fn from(value: Register) -> Self {
        Xmm(value)
    }
}

#[derive(Clone, Copy, Debug)]
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
