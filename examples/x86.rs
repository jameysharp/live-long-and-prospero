use geometry_compiler::codegen;
use geometry_compiler::ir;

fn main() -> ir::io::Result<()> {
    let memoized = ir::io::read(std::io::stdin().lock(), ir::memoize::MemoBuilder::new())?;
    codegen::x86::write(std::io::stdout().lock(), &memoized)?;
    Ok(())
}
