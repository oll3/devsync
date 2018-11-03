//extern crate crypto;
extern crate getopts;

use getopts::Options;
use std::cmp;
use std::env;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::prelude::*;
use std::io::{Seek, SeekFrom};
use std::process;

#[derive(Debug)]
struct Config {
    src: Option<String>,
    dest: String,
    block_size: usize,
    buf_size: usize,
    dry_run: bool,
}

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} <destination file> [options]", program);
    print!("{}", opts.usage(&brief));
    println!();
    println!("    SIZE values can be given in units 'TiB', 'GiB', 'MiB', 'KiB', or 'B' (default).");
    println!("    Will run until reaches end of file of either source or destionation.");
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
    opts.optopt(
        "u",
        "buffer-blocks",
        "number of blocks to buffer (default 1)",
        "BLOCKS",
    );
    opts.optopt(
        "s",
        "source",
        "source of synchronization (default is stdin)",
        "FILE",
    );
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
    let src = matches.opt_str("s");
    let block_size = if let Some(ref val) = matches.opt_str("b") {
        parse_size(val)
    } else {
        1024
    };
    let buf_blocks: usize = if let Some(ref val) = matches.opt_str("u") {
        val.parse().expect("blocks")
    } else {
        1
    };
    let dry_run = matches.opt_present("d");
    let dest = if matches.free.len() == 1 {
        matches.free[0].clone()
    } else {
        print_usage(&program, opts);
        process::exit(1);
    };

    Config {
        src: src,
        dest: dest,
        dry_run: dry_run,
        block_size: block_size,
        buf_size: buf_blocks * block_size,
    }
}

fn size_to_str(size: &usize) -> String {
    if *size > 1024 * 1024 {
        format!("{} MiB ({} bytes)", size / (1024 * 1024), size)
    } else if *size > 1024 {
        format!("{} KiB ({} bytes)", size / (1024), size)
    } else {
        format!("{} bytes", size)
    }
}

// Fill buffer with data from file, or until eof.
fn fill_buf<T>(file: &mut T, buf: &mut Vec<u8>) -> usize
where
    T: Read,
{
    let mut read_size = 0;
    let buf_size = buf.len();
    while read_size < buf_size {
        let rc = file.read(&mut buf[read_size..buf_size]).expect("read");
        if rc == 0 {
            break;
        } else {
            read_size += rc;
        }
    }
    return read_size;
}

// Compare and sync two files
fn sync_files<T>(config: &Config, src_file: &mut T, dest_file: &mut File)
where
    T: Read,
{
    let mut diff_blocks = 0;
    let mut diff_bytes = 0;
    let mut total_bytes = 0;
    let mut block_cnt = 0;

    // Pre-allocate the buffers to use
    let mut src_buf: Vec<u8> = vec![0; config.buf_size as usize];
    let mut dest_buf: Vec<u8> = vec![0; config.buf_size as usize];

    loop {
        // Fill both buffers to be used for comparsion.
        let src_buf_size = fill_buf(src_file, &mut src_buf);
        let dest_buf_size = fill_buf(dest_file, &mut dest_buf);
        if src_buf_size == 0 || dest_buf_size == 0 {
            // Reached eof for one of the files
            break;
        }

        let mut buf_offs = 0;
        while buf_offs < src_buf_size && buf_offs < dest_buf_size {
            // Iterate and compare 'block_size' bytes (at max) each time.
            // Only time the size of the source and the destination may differ
            // is when reading the last block of one of them.
            let src_cmp_size = cmp::min(config.block_size, src_buf_size - buf_offs);
            let dest_cmp_size = cmp::min(config.block_size, dest_buf_size - buf_offs);
            let cmp_size = cmp::min(src_cmp_size, dest_cmp_size);

            let src_slice = &src_buf[buf_offs..(buf_offs + cmp_size)];
            let dest_slice = &dest_buf[buf_offs..(buf_offs + cmp_size)];

            // Compare the two block buffers and write the soruce to the
            // destination file if they differ (and this is not a dry run).
            if src_slice != dest_slice {
                diff_blocks += 1;
                diff_bytes += cmp_size;

                if !config.dry_run {
                    // Store current position of dest file
                    let old_pos = dest_file.seek(SeekFrom::Current(0)).expect("seek current");

                    // jump back to block start
                    dest_file
                        .seek(SeekFrom::Start((block_cnt * config.block_size) as u64))
                        .expect("seek");

                    // write in data to dest file
                    let wc = dest_file.write(src_slice).expect("write");
                    if wc != cmp_size {
                        panic!("wc != cmp_size");
                    }

                    // restore position in dest file
                    dest_file
                        .seek(SeekFrom::Start(old_pos))
                        .expect("seek restore");
                }
            }

            block_cnt += 1;
            buf_offs += cmp_size;
            total_bytes += cmp_size;
        }
    }

    println!(
        "Compared {} blocks ({} in total).",
        block_cnt,
        size_to_str(&total_bytes)
    );
    println!(
        "{} blocks differed ({} in total) {} written to destination.",
        diff_blocks,
        size_to_str(&diff_bytes),
        if config.dry_run {
            "but was NOT"
        } else {
            "and was"
        }
    );
}

fn main() {
    let config = parse_opts();

    let mut dest_file = OpenOptions::new()
        .read(true)
        // Open with write access if not a 'dry run'
        .write(config.dry_run == false)
        .open(&config.dest)
        .expect(&format!("failed to open file ({})", config.dest));

    if let Some(val) = &config.src {
        // Read from input file
        let mut src_file = File::open(&val).expect(&format!("failed to open file ({})", val));
        sync_files(&config, &mut src_file, &mut dest_file);
    } else {
        // Read from stdin
        let stdin = io::stdin();
        let mut src_file = stdin.lock();
        sync_files(&config, &mut src_file, &mut dest_file);
    }
}
