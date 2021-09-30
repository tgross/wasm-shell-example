use std::convert::TryInto;
use std::io;

fn main() {
    loop {
        let mut user_input = String::new();
        io::stdin()
            .read_line(&mut user_input)
            .expect("error reading in user input");
        let result = eval(&user_input);
        println!("{:?}", result);
    }
}

// these are functions that we're importing from the host in a module
// called `host`; calling them is always `unsafe`
#[link(wasm_import_module = "host")]
extern "C" {
    fn host_add(count: i32);
    fn host_sum() -> i32;
    fn host_kv_get(ptr: u32, len: u32, res_ptr: u32, res_len: u32) -> u32;
    fn host_kv_set(
        key_ptr: u32,
        key_len: u32,
        val_ptr: u32,
        val_len: u32,
        res_ptr: u32,
        res_len: u32,
    ) -> u32;
}

fn eval(input: &str) -> String {
    let parsed: Vec<_> = input.trim_end().trim_start().split(' ').collect();
    match parsed.get(0) {
        Some(&"sum") => unsafe { format!("{}", host_sum()) },
        Some(&"add") => match parsed.get(1) {
            Some(next) => match str::parse(next) {
                Ok(i) => {
                    unsafe {
                        host_add(i);
                    }
                    "ok".to_string()
                }
                _ => "error: argument must be an integer".to_string(),
            },
            _ => {
                unsafe { host_add(1) }
                "ok".to_string()
            }
        },
        Some(&"get") => match parsed.get(1) {
            Some(next) => eval_kv_get(next),
            _ => "error: \"key\" needs an argument".to_string(),
        },
        Some(&"set") => match parsed.get(1) {
            Some(key) => match parsed.get(2) {
                Some(val) => eval_kv_set(key, val),
                _ => "error: \"key\" needs 2 arguments".to_string(),
            },
            _ => "error: \"key\" needs 2 arguments".to_string(),
        },

        _ => input.to_string(),
    }
}

const MAX_RESPONSE_LENGTH: usize = 1024;

// eval_kv writes the key to our buffer and calls into the host to
// overwrite that buffer with the return value; the host will grow the
// buffer as needed and return the length of bytes written
fn eval_kv_get(key: &str) -> String {
    let key_len = key.len();
    let res;

    unsafe {
        let key_ptr = alloc(key.len());
        std::ptr::copy(key.as_ptr(), key_ptr, key_len);

        let res_ptr = alloc(MAX_RESPONSE_LENGTH);

        let _res_len = host_kv_get(
            key_ptr as u32,
            key_len as u32,
            res_ptr as u32,
            MAX_RESPONSE_LENGTH as u32,
        );

        let res_len = _res_len.try_into().unwrap();
        res = read_results(res_ptr, res_len);

        // free our forgotten memory for the key; the from_utf8 will
        // free the response buffer
        dealloc(key_ptr, key_len);
    }
    match std::str::from_utf8(&res) {
        Ok(s) => s.to_string(),
        Err(err) => format!("error parsing results as string: {:?}", err),
    }
}

fn alloc(len: usize) -> *mut u8 {
    let mut buf = vec![0u8; len];
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

unsafe fn dealloc(ptr: *mut u8, len: usize) {
    let _buf = Vec::from_raw_parts(ptr, 0, len);
    std::mem::drop(_buf);
}

// eval_kv writes the key to our buffer and calls into the host to
// overwrite that buffer with the return value; the host will grow the
// buffer as needed and return the length of bytes written
fn eval_kv_set(key: &str, val: &str) -> String {
    let key_len = key.len();
    let val_len = val.len();
    let res;

    unsafe {
        let key_ptr = alloc(key.len());
        std::ptr::copy(key.as_ptr(), key_ptr, key_len);

        let val_ptr = alloc(val.len());
        std::ptr::copy(val.as_ptr(), val_ptr, val_len);

        let res_ptr = alloc(MAX_RESPONSE_LENGTH);

        let _res_len = host_kv_set(
            key_ptr as u32,
            key_len as u32,
            val_ptr as u32,
            val_len as u32,
            res_ptr as u32,
            MAX_RESPONSE_LENGTH as u32,
        );

        let res_len = _res_len.try_into().unwrap();
        res = read_results(res_ptr, res_len);

        // free our forgotten memory for the key and value; the
        // from_utf8 will free the response buffer
        dealloc(key_ptr, key_len);
        dealloc(val_ptr, val_len);
    }
    match std::str::from_utf8(&res) {
        Ok(s) => s.to_string(),
        Err(err) => format!("error parsing results as string: {:?}", err),
    }
}

fn read_results(ptr: *mut u8, len: usize) -> Vec<u8> {
    unsafe { Vec::from_raw_parts(ptr, len, len) }
}
