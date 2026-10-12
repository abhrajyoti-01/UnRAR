//! UnRAR — entry point.
//!
//! Order: (1) if this exe carries an appended ZIP payload (SFX built by
//! "Create SFX"), self-extract and exit; (2) CLI subcommands when args are
//! given (l/t/x/a/i); (3) the GUI otherwise / with an archive path.

#![windows_subsystem = "windows"]

mod app;
mod engine;
mod icons;
mod viewer;

use std::path::PathBuf;

fn main() {
    // SFX payload? self-extract and exit.
    if engine::self_extract() {
        return;
    }

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "l" | "t" | "x" | "a" | "i" => {
                attach_console();
                std::process::exit(cli(&args));
            }
            _ => {}
        }
    }
    let start = args.get(1).map(PathBuf::from)
        .filter(|p| p.exists());
    let _ = app::run(start);
}

/// Attach a windows-subsystem exe to the caller's console so CLI works.
#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    extern "system" { fn AttachConsole(pid: u32) -> i32; }
    unsafe { AttachConsole(0xFFFF_FFFF); }   // ATTACH_PARENT_PROCESS
}

#[cfg(not(windows))]
fn attach_console() {}

fn cli(args: &[String]) -> i32 {
    let cmd = args[1].as_str();
    let Some(arc) = args.get(2).map(PathBuf::from) else {
        eprintln!("usage: UnRAR l|t|x|a|i <archive> [args]");
        return 2;
    };
    match cmd {
        "l" => match engine::list_archive(&arc) {
            Ok(info) => {
                println!("{}  {}", info.format.label(), arc.display());
                for e in &info.entries {
                    let crc = e.crc.map(|c| format!("{:08X}", c))
                        .unwrap_or_default();
                    println!("{:<48} {:>10} {:>10} {:>5} {:>16} {:>8}  {}",
                             e.name,
                             if e.is_dir { "<DIR>".into() }
                                 else { fmt_size_cli(e.size) },
                             if e.is_dir { "".into() }
                                 else { fmt_size_cli(e.packed) },
                             e.ratio(),
                             e.mtime.clone().unwrap_or_default(),
                             crc, e.method);
                }
                println!("{} entries", info.entries.len());
                0
            }
            Err(e) => { eprintln!("error: {e}"); 1 }
        },
        "t" => match engine::test_archive(&arc) {
            Ok(s) => { println!("{s}"); 0 }
            Err(e) => { eprintln!("{e}"); 1 }
        },
        "x" => {
            let dest = args.get(3).map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(
                    format!("{}_extracted",
                            arc.file_stem().unwrap_or_default()
                                .to_string_lossy())));
            match engine::extract_archive(&arc, &dest, None, None) {
                Ok(n) => { println!("Extracted {n} item(s) -> {}", dest.display()); 0 }
                Err(e) => { eprintln!("error: {e}"); 1 }
            }
        }
        "a" => {
            let files: Vec<PathBuf> =
                args[3..].iter().map(PathBuf::from).collect();
            let name = arc.to_string_lossy().to_lowercase();
            let res: anyhow::Result<String> = if name.ends_with(".rar") {
                engine::rar_create(&arc, &files, &engine::RarOpts {
                    level: 3, ..Default::default()
                }).map(|s| s)
            } else if name.ends_with(".tar") || name.ends_with(".tar.gz")
                || name.ends_with(".tgz") || name.ends_with(".tar.bz2")
                || name.ends_with(".tar.xz") || name.ends_with(".tar.zst") {
                let fmt = if name.contains("gz") || name.ends_with("tgz") {
                    engine::Format::Gz
                } else if name.contains("bz2") {
                    engine::Format::Bz2
                } else if name.contains("xz") {
                    engine::Format::Xz
                } else if name.contains("zst") {
                    engine::Format::Zst
                } else { engine::Format::Tar };
                engine::tar_create(&arc, &files, fmt)
                    .map(|n| format!("TAR created ({n} items)"))
            } else {
                engine::zip_add(&arc, &files, 6, None, None)
                    .map(|n| format!("Added {n} file(s) to {}", arc.display()))
            };
            match res {
                Ok(s) => { println!("{s}"); 0 }
                Err(e) => { eprintln!("error: {e}"); 1 }
            }
        }
        "i" => match engine::list_archive(&arc) {
            Ok(i) => {
                println!("Archive: {}", arc.display());
                println!("Format:  {}", i.format.label());
                println!("Entries: {}", i.entries.len());
                let t: u64 = i.entries.iter().map(|e| e.size).sum();
                let p: u64 = i.entries.iter().map(|e| e.packed).sum();
                println!("Unpacked: {}", fmt_size_cli(t));
                println!("Packed:   {}", fmt_size_cli(p));
                0
            }
            Err(e) => { eprintln!("error: {e}"); 1 }
        },
        _ => 2,
    }
}

fn fmt_size_cli(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut f = n as f64;
    for (i, u) in U.iter().enumerate() {
        if f < 1024.0 || i == 4 {
            return if i == 0 { format!("{n} {u}") }
                   else { format!("{f:.1} {u}") };
        }
        f /= 1024.0;
    }
    String::new()
}
