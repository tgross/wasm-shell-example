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

fn eval(input: &str) -> &str {
    input
}
