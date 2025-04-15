use geometry_compiler::ir;

fn main() -> ir::io::Result<()> {
    let mut insts = ir::io::read(std::io::stdin().lock())?;
    ir::reassociate::reassociate(&mut insts);
    ir::reorder::reorder(&mut insts);
    ir::io::write(std::io::stdout().lock(), insts.iter().cloned())?;
    Ok(())
}
