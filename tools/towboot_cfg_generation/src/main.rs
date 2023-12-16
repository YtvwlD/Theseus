//! Parses the directory of built object files
//! and generates a towboot.toml file.


extern crate getopts;

use getopts::Options;
use std::fs;
use std::io::Write;
use std::process;
use std::env;

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();

    let mut opts = Options::new();
    opts.optopt("o", "", "set output file path, e.g., \"/my/dir/towboot.toml\"", "OUTPUT_PATH");
    opts.optflag("h", "help", "print this help menu");

    let matches = opts.parse(&args[1..]).map_err(|e| e.to_string())?;

    if matches.opt_present("h") {
        print_usage("cargo run -- ", opts);
        process::exit(0);
    }

    // Require input directory 
    let input_directory = match matches.free.len() {
        0 => return Err(format!("no input directory provided")),
        1 => matches.free[0].clone(), 
        _ => return Err(format!("Too many arguments entered")),
    };
    
    let towboot_cfg_string = create_towboot_cfg_string(input_directory)?;
    
    // Write to output file (if provided) 
    if matches.opt_present("o") {
        let output_file_path = matches.opt_str("o")
            .ok_or_else(|| String::from("failed to match output file argument."))?;
        write_content(towboot_cfg_string, output_file_path);
    }
    // Otherwise, write to stdout by default
    else {
        println!("{}", towboot_cfg_string);
    }

    Ok(())
}

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options] INPUT_DIRECTORY", program);
    print!("{}", opts.usage(&brief));
}

fn create_towboot_cfg_string(input_directory: String) -> Result<String, String> {
    // Creates string to write to grub.cfg file by looking through all files in input_directory
    let mut content = String::new();
    
    let mut path_to_exe = std::env::current_exe().unwrap_or_default();
    // go up three directories to remove the "target/<build_mode>/name"
    path_to_exe.pop(); path_to_exe.pop(); path_to_exe.pop();

    // TODO: use a TOML library for this
    content.push_str("### This file has been autogenerated, do not manually modify it!\n");
    content.push_str(&format!("### Generated by program: \"{}\"\n", path_to_exe.display()));
    content.push_str(&format!("### Input directory: \"{}\"\n\n", &input_directory));
    content.push_str("default = \"theseus\"\n");
    content.push_str("timeout = 0\n");
    content.push_str("[entries]\n");
    content.push_str("[entries.theseus]\n");
    content.push_str("\tname = \"Theseus OS\"\n");
    content.push_str("\timage = \"boot/kernel.bin\"\n");

    for path in fs::read_dir(input_directory).map_err(|e| e.to_string())? {
        let path = path.map_err(|e| e.to_string())?;
        let p = path.path();
        let file_name = p.file_name().and_then(|f| f.to_str()).ok_or_else(|| format!("Path error in path {:?}", path))?;
        content.push_str("\t[[entries.theseus.modules]]\n");
        content.push_str(&format!("\t\timage = \"modules/{file_name}\"\n"));
        content.push_str(&format!("\t\targv  = \"{file_name}\"\n"));
    }
    Ok(content)
}

fn write_content(content: String, output_file_path: String) {
    if let Ok(mut file) = fs::File::create(output_file_path) {
        if file.write(content.as_bytes()).is_ok(){ process::exit(0); }
    }
}
