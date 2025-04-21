use geometry_compiler::ir;

fn main() -> ir::io::Result<()> {
    let insts = ir::io::read(std::io::stdin().lock(), ir::Insts::default())?;
    let memoized = ir::memoize::memoize(&insts);
    ir::io::write_memoized(std::io::stdout().lock(), &memoized)?;
    Ok(())
}
