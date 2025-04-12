use geometry_compiler::ir::{reader, writer};

fn main() -> reader::Result<()> {
    let insts = reader::read(std::io::stdin().lock())?;
    writer::write(std::io::stdout().lock(), insts.iter())?;
    Ok(())
}
