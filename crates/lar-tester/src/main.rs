//! LAR Test Harness CLI Entry Point.

use lar_tester::qemu::QemuEnvironment;
use lar_tester::runner::LarTestHarness;
use std::env;
use std::process;

fn print_usage() {
    println!("LAR Test Runner & QEMU/APK Validator");
    println!("Usage:");
    println!("  lar-tester --self-test                 Run synthetic end-to-end self tests");
    println!("  lar-tester --apk <path/to/app.apk>     Extract and test all ARM64 libraries in an APK");
    println!("  lar-tester --lib <path/to/lib.so>      Test a standalone ARM64 .so library");
    println!("  lar-tester --check-qemu                Check QEMU aarch64 environment and sysroots");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 || args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    match args[1].as_str() {
        "--self-test" => {
            println!("[LAR-TESTER] Running synthetic self-test suite...");
            let report = LarTestHarness::run_self_test();
            println!("{}", report);
            if !report.is_success() {
                process::exit(1);
            }
        }
        "--check-qemu" => {
            println!("[LAR-TESTER] Detecting QEMU environment...");
            let qemu = QemuEnvironment::detect();
            println!("  QEMU Binary : {:?}", qemu.qemu_path);
            println!("  Sysroot     : {:?}", qemu.sysroot_path);
            println!("  Available   : {}", qemu.is_available);
        }
        "--apk" => {
            if args.len() < 3 {
                eprintln!("Error: Missing APK file path. Usage: lar-tester --apk <path.apk>");
                process::exit(1);
            }
            let apk_path = &args[2];
            println!("[LAR-TESTER] Testing APK: {}", apk_path);
            match LarTestHarness::test_apk(apk_path) {
                Ok(report) => {
                    println!("{}", report);
                    if !report.is_success() {
                        process::exit(1);
                    }
                }
                Err(err) => {
                    eprintln!("[LAR-TESTER] Error: {}", err);
                    process::exit(1);
                }
            }
        }
        "--lib" => {
            if args.len() < 3 {
                eprintln!("Error: Missing .so file path. Usage: lar-tester --lib <path.so>");
                process::exit(1);
            }
            let so_path = &args[2];
            println!("[LAR-TESTER] Testing SO library: {}", so_path);
            match LarTestHarness::test_so_file(so_path) {
                Ok(report) => {
                    println!("{}", report);
                    if !report.is_success() {
                        process::exit(1);
                    }
                }
                Err(err) => {
                    eprintln!("[LAR-TESTER] Error: {}", err);
                    process::exit(1);
                }
            }
        }
        other => {
            eprintln!("Unknown option: {}", other);
            print_usage();
            process::exit(1);
        }
    }
}
