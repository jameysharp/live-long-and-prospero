use std::io::Write;

use geometry_compiler::codegen;
use geometry_compiler::ir;

fn main() -> ir::io::Result<()> {
    let insts = ir::io::read(std::io::stdin().lock())?;
    let memoized = ir::memoize::memoize(&insts);

    let mut out = std::io::stdout().lock();
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
    writeln!(out, "stride: .short {}", codegen::x86::STRIDE)?;

    for func in memoized.funcs.iter() {
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
            let (insts, stack_slots) =
                codegen::regalloc::alloc(&func.insts, func.vars, &func.outputs);
            codegen::x86::write(
                &mut out,
                insts.into_iter().rev(),
                stack_slots,
                [func.vars, ir::Var::X.into()],
            )?;
        }
    }

    Ok(())
}
