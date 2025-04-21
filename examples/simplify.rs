use geometry_compiler::ir;

fn main() -> ir::io::Result<()> {
    let insts = ir::io::read(std::io::stdin().lock())?;
    ir::io::write(std::io::stdout().lock(), insts.pool.iter().cloned())?;
    Ok(())
}
