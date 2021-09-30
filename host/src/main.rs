use std::collections::HashMap;
use std::convert::TryInto;
use std::env;
use std::io::prelude::*;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Result};
use wasi_common::pipe::{ReadPipe, WritePipe};
use wasmtime::*;
use wasmtime_wasi::sync::WasiCtxBuilder;
use wasmtime_wasi::{WasiCtx, WasiFile};

// Memory is by default exported from Rust modules under the
// name `memory`. This can be tweaked with the -Clink-arg flag
// to rustc to pass flags to LLD, the WebAssembly code linker.
const MEMORY: &str = &"memory";
const MAX_PARAMETER_SIZE: u32 = 1024;

struct State {
    counts: Vec<i32>,
    map: HashMap<String, String>,
}

impl State {
    fn new() -> State {
        State {
            counts: Vec::new(),
            map: HashMap::new(),
        }
    }

    fn add(&mut self, val: i32) {
        self.counts.push(val);
    }

    fn sum(&self) -> i32 {
        self.counts.iter().fold(0, |mut sum, &x| {
            sum += x;
            sum
        })
    }
}

struct StoreData<'a> {
    state: &'a Arc<Mutex<State>>,
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

    linker.func_wrap(
        "host",
        "host_add",
        |mut caller: Caller<'_, StoreData>, param: i32| {
            caller.data_mut().state.lock().unwrap().add(param);
        },
    )?;

    linker.func_wrap(
        "host",
        "host_sum",
        |mut caller: Caller<'_, StoreData>| -> i32 {
            caller.data_mut().state.lock().unwrap().sum()
        },
    )?;

    linker.func_wrap(
        "host",
        "host_kv_set",
        |mut caller: Caller<'_, StoreData>,
         key_ptr: u32,
         key_len: u32,
         val_ptr: u32,
         val_len: u32,
         res_ptr: u32,
         res_len: u32|
         -> u32 {
            let mem = match caller.get_export(MEMORY) {
                Some(Extern::Memory(mem)) => mem,
                _ => {
                    eprintln!("module did not have a memory export");
                    return 0;
                }
            };
            let store = caller.as_context();
            let result = kv_set(
                mem, &store, key_ptr, key_len, val_ptr, val_len, res_ptr, res_len,
            );
            let mut store = caller.as_context_mut();
            match result {
                Ok(response) => {
                    return write_response(mem, &mut store, res_ptr, res_len, response)
                        .map_err(|err| eprintln!("{}", err))
                        .unwrap_or(0);
                }
                Err(err) => {
                    let response = format!("{}", err);
                    eprintln!("{}", err);
                    return write_response(mem, &mut store, res_ptr, res_len, response)
                        .map_err(|err| eprintln!("{}", err))
                        .unwrap_or(0);
                }
            }
        },
    )?;

    linker.func_wrap(
        "host",
        "host_kv_get",
        |mut caller: Caller<'_, StoreData>,
         key_ptr: u32,
         key_len: u32,
         res_ptr: u32,
         res_len: u32|
         -> u32 {
            let mem = match caller.get_export(MEMORY) {
                Some(Extern::Memory(mem)) => mem,
                _ => {
                    eprintln!("module did not have a memory export");
                    return 0;
                }
            };
            let store = caller.as_context();
            let result = kv_get(mem, &store, key_ptr, key_len, res_ptr, res_len);
            let mut store = caller.as_context_mut();
            match result {
                Ok(response) => {
                    return write_response(mem, &mut store, res_ptr, res_len, response)
                        .map_err(|err| eprintln!("{}", err))
                        .unwrap_or(0);
                }
                Err(err) => {
                    let response = format!("{}", err);
                    eprintln!("{}", err);
                    return write_response(mem, &mut store, res_ptr, res_len, response)
                        .map_err(|err| eprintln!("{}", err))
                        .unwrap_or(0);
                }
            }
        },
    )?;

    wasmtime_wasi::add_to_linker(&mut linker, |data: &mut StoreData| &mut data.wasi)?;

    println!("compiling wasm module...");
    let module = Module::from_file(&engine, module_path)?;

    let state = Arc::new(Mutex::new(State::new()));

    println!("starting server...");
    let listener = UnixListener::bind(bind_path)?;
    let linker = Arc::new(linker);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = state.clone();
                let engine = engine.clone();
                let module = module.clone();
                let linker = linker.clone();
                thread::spawn(move || handle_client(stream, &engine, &module, &linker, &state));
            }
            Err(_) => {
                eprintln!("connection failed");
                break;
            }
        }
    }

    Ok(())
}

fn handle_client(
    stream: UnixStream,
    engine: &Engine,
    module: &Module,
    linker: &Linker<StoreData>,
    state: &Arc<Mutex<State>>,
) {
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

    let mut store = Store::new(
        &engine,
        StoreData {
            state: state,
            wasi: wasi,
        },
    );

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

fn read_parameter(
    mem: Memory,
    store: &StoreContext<StoreData>,
    ptr: u32,
    len: u32,
) -> Result<String> {
    validate_wasm_param(ptr, len)?;
    let mut buf = vec![0u8; len as usize];
    mem.read(&store, ptr.try_into()?, &mut buf)?;
    Ok(std::str::from_utf8(&buf)?.to_string())
}

fn write_response(
    mem: Memory,
    store: &mut StoreContextMut<StoreData>,
    ptr: u32,
    max_len: u32,
    mut response: String,
) -> Result<u32> {
    response.truncate(max_len.try_into()?);
    mem.write(store, ptr.try_into()?, response.as_bytes())?;
    Ok(response.len() as u32)
}

fn validate_wasm_param(ptr: u32, len: u32) -> Result<()> {
    if ptr == 0 || len == 0 {
        return Err(anyhow!("pointer and length need must be non-zero"));
    }
    if len > MAX_PARAMETER_SIZE {
        return Err(anyhow!("parameter exceed maximum length"));
    }
    Ok(())
}

fn kv_set(
    mem: Memory,
    store: &StoreContext<StoreData>,
    key_ptr: u32,
    key_len: u32,
    val_ptr: u32,
    val_len: u32,
    res_ptr: u32,
    res_len: u32,
) -> Result<String> {
    let key = read_parameter(mem, &store, key_ptr, key_len)?;
    let val = read_parameter(mem, &store, val_ptr, val_len)?;
    validate_wasm_param(res_ptr, res_len)?;
    let max_len: usize = res_len.try_into().unwrap_or(1024);

    store.data().state.lock().unwrap().map.insert(key, val);
    let mut response = "ok".to_string();
    response.truncate(max_len);
    Ok(response)
}

fn kv_get(
    mem: Memory,
    store: &StoreContext<StoreData>,
    key_ptr: u32,
    key_len: u32,
    res_ptr: u32,
    res_len: u32,
) -> Result<String> {
    let key = read_parameter(mem, &store, key_ptr, key_len)?;
    validate_wasm_param(res_ptr, res_len)?;
    let max_len: usize = res_len.try_into().unwrap_or(1024);

    let map = &store.data().state.lock().unwrap().map;
    let mut response = map.get(&key).ok_or(anyhow!("no such key"))?.to_string();
    response.truncate(max_len);
    Ok(response)
}
