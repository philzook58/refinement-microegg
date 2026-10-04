#[cfg(not(target_arch = "wasm32"))]
use clap::{Arg, Command};
#[cfg(not(target_arch = "wasm32"))]
use std::{io::Read, path::PathBuf};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_wasm(input: &str) -> Result<String, JsValue> {
    refinement_microegg::script::run(input)
        .map(|lines| lines.join("\n"))
        .map_err(|error| JsValue::from_str(&error))
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let matches = Command::new("refinement-microegg")
        .about("Run non-binder lambda-microegg S-expression scripts")
        .arg(
            Arg::new("file")
                .help("Script file, or - for standard input")
                .default_value("-"),
        )
        .get_matches();
    let file = PathBuf::from(matches.get_one::<String>("file").unwrap());
    let result = (|| -> Result<(), String> {
        let mut input = String::new();
        if file.to_string_lossy() == "-" {
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
        } else {
            input =
                std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        }
        for line in refinement_microegg::script::run(&input)? {
            println!("{line}");
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
