use std::io::Write;

use geometry_compiler::{codegen, ir};

fn main() -> ir::io::Result<()> {
    let insts = ir::io::read(std::io::stdin().lock())?;
    let memoized = ir::memoize::memoize(&insts);

    let mut out = std::io::stdout().lock();
    for func in memoized.funcs.iter() {
        if !func.insts.is_empty() {
            writeln!(out, "global {:?}", func.vars)?;
            writeln!(out, "{:?}:", func.vars)?;
            let (insts, stack_slots) =
                codegen::regalloc::alloc(&func.insts, func.vars, &func.outputs);
            codegen::x86::write(&mut out, insts.into_iter().rev(), stack_slots)?;
            writeln!(out)?;
        }
    }

    writeln!(out, "[section .rodata]")?;
    writeln!(out, "global consts")?;
    writeln!(out, "consts:")?;
    for value in memoized.consts.iter() {
        writeln!(out, "dd {:#08x}", value.bits())?;
    }

    Ok(())
}
