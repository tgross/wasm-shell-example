use std::env;

use anyhow::{anyhow, Result};
use wasmtime::*;
use wasmtime_wasi::sync::WasiCtxBuilder;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err(anyhow!("need to supply the wasm module path"));
    }
    let module_path = args[1].clone();

    let engine = Engine::default();
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::add_to_linker(&mut linker, |s| s)?;

    let wasi = WasiCtxBuilder::new().inherit_stdio().build();
    let mut store = Store::new(&engine, wasi);

    println!("compiling wasm module...");
    let module = Module::from_file(&engine, module_path)?;
    linker.module(&mut store, "", &module)?;

    println!("starting interpreter...");

    linker
        .get_default(&mut store, "")?
        .typed::<(), (), _>(&store)?
        .call(&mut store, ())?;

    Ok(())
}
