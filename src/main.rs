//! LAR - Linux Android Runtime CLI Entry Point.

use lar::memory::page::host_page_size;
use lar::LarRuntime;
use std::env;
use std::process;

fn print_banner(runtime: &LarRuntime) {
    println!("========================================================");
    println!("       LAR - Linux Android Runtime (Phase 0)            ");
    println!("========================================================");
    println!("Host Architecture   : {}", runtime.host_arch);
    println!("Execution Mode      : {:?}", runtime.execution_mode);
    println!("Host Page Size      : {} KB ({} bytes)", host_page_size() / 1024, host_page_size());
    println!("Guest Page Target   : 16 KB (16384 bytes)");
    println!("Bionic Shims Loaded : {} symbols", runtime.symbol_registry.count());
    println!("========================================================\n");
}

fn main() {
    let mut runtime = LarRuntime::new();
    print_banner(&runtime);

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: lar <path-to-arm64-library.so> [options]");
        println!("Options:");
        println!("  --info       Inspect ELF headers and segment mappings without executing");
        println!("  --symbols    List resolved and registered symbols");
        return;
    }

    let library_path = &args[1];
    println!("[LAR] Loading ARM64 shared library: {}", library_path);

    match runtime.load_library_file(library_path) {
        Ok(lib) => {
            println!("[LAR] Successfully loaded '{}'!", lib.name);
            println!("      Load Base Address : 0x{:x}", lib.load_base);
            println!("      Mapped Size       : {} bytes", lib.mem_region.len());
            println!("      Init Routines     : {} found", lib.init_array.len());
            println!("      Entry Point       : {:?}", lib.entry_point.map(|e| format!("0x{:x}", e)));

            if args.iter().any(|arg| arg == "--symbols") {
                println!("\n[LAR] Exported Symbols:");
                // Sample some symbols
                if let Some(malloc_addr) = runtime.resolve_symbol("malloc") {
                    println!("  malloc -> 0x{:x}", malloc_addr);
                }
                if let Some(log_addr) = runtime.resolve_symbol("__android_log_print") {
                    println!("  __android_log_print -> 0x{:x}", log_addr);
                }
            }
        }
        Err(err) => {
            eprintln!("[LAR] Error loading library: {}", err);
            process::exit(1);
        }
    }
}
