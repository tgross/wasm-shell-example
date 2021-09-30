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
        _ => input.to_string(),
    }
}
