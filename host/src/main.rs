use std::env;
use std::io::prelude::*;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::thread;

use anyhow::{anyhow, Result};
use wasi_common::pipe::{ReadPipe, WritePipe};
use wasmtime::*;
use wasmtime_wasi::sync::WasiCtxBuilder;
use wasmtime_wasi::{WasiCtx, WasiFile};

struct StoreData {
    wasi: WasiCtx,
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        return Err(anyhow!("need to supply the wasm module path and bind path"));
    }
    let module_path = args[1].clone();
    let bind_path = args[2].clone();

    let engine = Engine::default();
    let mut linker = Linker::new(&engine);

    wasmtime_wasi::add_to_linker(&mut linker, |data: &mut StoreData| &mut data.wasi)?;

    println!("compiling wasm module...");
    let module = Module::from_file(&engine, module_path)?;

    println!("starting server...");
    let listener = UnixListener::bind(bind_path)?;
    let linker = Arc::new(linker);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let engine = engine.clone();
                let module = module.clone();
                let linker = linker.clone();
                thread::spawn(move || handle_client(stream, &engine, &module, &linker));
            }
            Err(_) => {
                eprintln!("connection failed");
                break;
            }
        }
    }

    Ok(())
}

fn handle_client(stream: UnixStream, engine: &Engine, module: &Module, linker: &Linker<StoreData>) {
    println!("starting interpreter...");

    let mut err_stream = match stream.try_clone() {
        Ok(err_writer) => err_writer,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    let write_stream = match stream.try_clone() {
        Ok(writer) => writer,
        Err(e) => {
            if let Err(e) = write!(&mut err_stream, "{}", e) {
                eprintln!("{}", e); // not much else we can do at this point
            }
            return;
        }
    };

    let wasi = WasiCtxBuilder::new()
        .stdin(Box::new(ReadPipe::new(stream)) as Box<dyn WasiFile>)
        .stdout(Box::new(WritePipe::new(write_stream)) as Box<dyn WasiFile>)
        .build();

    let mut store = Store::new(&engine, StoreData { wasi: wasi });

    let mut run_interpreter = move || -> Result<(), Trap> {
        let instance = linker.instantiate(&mut store, module)?;
        let run = instance.get_typed_func::<(), (), _>(&mut store, "_start")?;
        run.call(&mut store, ())
    };

    if let Err(e) = run_interpreter() {
        if let Err(e) = write!(&mut err_stream, "{}", e) {
            eprintln!("{}", e);
        }
        return;
    };
}
