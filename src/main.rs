//extern crate crypto;
extern crate getopts;

use getopts::Options;
use std::cmp;
use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::io::{Seek, SeekFrom};
use std::process;

#[derive(Debug)]
struct Config {
    input: String,
    output: String,
    block_size: usize,
    buf_size: usize,
    dry_run: bool,
}

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} <input> <output> [options]", program);
    print!("{}", opts.usage(&brief));
    println!();
    println!("    SIZE values can be given in units 'TiB', 'GiB', 'MiB', 'KiB', or 'B' (default).");
    println!();
}

fn parse_size(size_str: &str) -> usize {
    let size_val: String = size_str.chars().filter(|a| a.is_numeric()).collect();
    let size_val: usize = size_val.parse().expect("parse");
    let size_unit: String = size_str.chars().filter(|a| !a.is_numeric()).collect();
    if size_unit.len() == 0 {
        return size_val;
    }
    match size_unit.as_str() {
        "TiB" => 1024 * 1024 * 1024 * 1024 * size_val,
        "GiB" => 1024 * 1024 * 1024 * size_val,
        "MiB" => 1024 * 1024 * size_val,
        "KiB" => 1024 * size_val,
        "B" => size_val,
        _ => panic!("Invalid size unit"),
    }
}

fn parse_opts() -> Config {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();
    let mut opts = Options::new();

    opts.optopt("b", "block-size", "set block-size (default 1KiB)", "SIZE");
    opts.optopt("s", "buffer-size", "set buffer-size (default 1KiB)", "SIZE");
    opts.optflag("d", "dry-run", "compare but do not write");
    opts.optflag("h", "help", "print this help menu");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => panic!(f.to_string()),
    };
    if matches.opt_present("h") {
        print_usage(&program, opts);
        process::exit(1);
    }
    let block_size = if let Some(ref val) = matches.opt_str("b") {
        parse_size(val)
    } else {
        1024
    };
    let buf_size = if let Some(ref val) = matches.opt_str("s") {
        parse_size(val)
    } else {
        1024
    };
    if buf_size < block_size {
        panic!("buffer size < block size");
    }
    let dry_run = matches.opt_present("d");
    let (input, output) = if matches.free.len() == 2 {
        (matches.free[0].clone(), matches.free[1].clone())
    } else {
        print_usage(&program, opts);
        process::exit(1);
    };

    Config {
        input: input,
        output: output,
        dry_run: dry_run,
        block_size: block_size,
        buf_size: buf_size,
    }
}

fn min_file_size(in_file: &mut File, out_file: &mut File) -> u64 {
    let ir = in_file.seek(SeekFrom::End(0)).expect("seek in file");
    let or = out_file.seek(SeekFrom::End(0)).expect("seek out file");
    println!("ir={:?}, or={:?}", ir, or);
    in_file.seek(SeekFrom::Start(0)).expect("seek in file");
    out_file.seek(SeekFrom::Start(0)).expect("seek out file");

    return cmp::min(ir, or);
}

struct BlockDesc {
    block: usize,
    offset: std::io::SeekFrom,
    size: usize,
}

fn size_to_str(size: &usize) -> String {
    if *size > 1024 * 1024 {
        format!("{} MiB ({} B)", size / (1024 * 1024), size)
    } else if *size > 1024 {
        format!("{} KiB ({} B)", size / (1024), size)
    } else {
        format!("{} B", size)
    }
}

// Compare two files and return list of blocks that differ between them
fn diff_file(config: &Config, in_file: &mut File, out_file: &mut File) -> Vec<BlockDesc> {
    let mut diff_bytes = 0;
    let mut total_bytes = 0;
    let mut differ = vec![];
    let mut block_cnt = 0;
    let mut in_buf: Vec<u8> = vec![0; config.buf_size as usize];
    let mut out_buf: Vec<u8> = vec![0; config.buf_size as usize];

    loop {
        let mut buf_offs = 0;

        // Read at max 'buf_size' bytes from file
        let in_rc = in_file.read(&mut in_buf).expect("read input file");
        let out_rc = out_file.read(&mut out_buf).expect("read input file");
        if in_rc == 0 || out_rc == 0 {
            // Reached eof for one of the files
            break;
        }

        while buf_offs < in_rc && buf_offs < out_rc {
            // Iterate and compare 'block_size' bytes (at max) each time
            let in_cmp_size = cmp::min(config.block_size, in_rc - buf_offs);
            let out_cmp_size = cmp::min(config.block_size, out_rc - buf_offs);
            let cmp_size = cmp::min(in_cmp_size, out_cmp_size);

            let in_slice = &in_buf[buf_offs..buf_offs + cmp_size];
            let out_slice = &out_buf[buf_offs..buf_offs + cmp_size];

            let block_desc = BlockDesc {
                block: block_cnt,
                offset: SeekFrom::Start(block_cnt as u64 * config.block_size as u64),
                size: cmp_size,
            };

            // Compare block
            if in_slice != out_slice {
                differ.push(block_desc);
                diff_bytes += cmp_size;
            }

            block_cnt += 1;
            buf_offs += cmp_size;
            total_bytes += cmp_size;
        }
    }

    println!(
        "Compared {} blocks, {} in total.",
        block_cnt,
        size_to_str(&total_bytes)
    );
    println!(
        "{} blocks differs, {} in total.",
        differ.len(),
        size_to_str(&diff_bytes)
    );
    return differ;
}

fn main() {
    let config = parse_opts();
    println!("config={:?}", config);

    let mut in_file =
        File::open(&config.input).expect(&format!("failed to open file ({})", config.input));
    let mut out_file =
        File::open(&config.output).expect(&format!("failed to open file ({})", config.input));

    println!(
        "Comparing blocks (size: {}) between '{}' and '{}'",
        size_to_str(&config.block_size),
        config.input,
        config.output
    );
    let diff = diff_file(&config, &mut in_file, &mut out_file);
}
