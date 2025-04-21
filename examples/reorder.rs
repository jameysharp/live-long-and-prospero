use geometry_compiler::ir;

fn main() -> ir::io::Result<()> {
    let mut insts = ir::io::read(std::io::stdin().lock(), ir::Insts::default())?;
    ir::reorder::reorder(&mut insts);
    ir::io::write(std::io::stdout().lock(), insts.pool.iter().cloned())?;
    Ok(())
}
