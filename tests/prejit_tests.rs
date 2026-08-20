//! Unit and Integration Tests for Install-Time Pre-JIT and Callgraph Analyzer.

use lar::LarRuntime;
use lar::arch::Arm64CpuContext;
use lar::linker::*;
use lar::prejit::*;
use lar_tester::synthetic::generate_synthetic_arm64_so;

#[test]
fn test_prejit_callgraph_extraction_and_precompilation() {
    // 1. Generate synthetic ARM64 library with DT_INIT and JNI_OnLoad
    let elf_bytes = generate_synthetic_arm64_so("libgame_core.so", "JNI_OnLoad");
    let mut registry = SymbolRegistry::new();
    lar::bionic::register_bionic_shims(&mut registry);

    let loaded = ElfLoader::load_from_memory("libgame_core.so", &elf_bytes, &mut registry)
        .expect("Failed to load library");
    let compiled_base = loaded.load_base;

    // 2. Build startup callgraph
    let nodes = CallgraphAnalyzer::build_startup_graph(&loaded);
    assert!(!nodes.is_empty());
    assert!(
        nodes
            .iter()
            .any(|n| n.name.contains("DT_INIT") || n.name == "JNI_OnLoad")
    );

    // 3. Run Pre-JIT Daemon compilation to disk cache
    let temp_dir = std::env::temp_dir();
    let cache_path = temp_dir.join("libgame_core.larcache");

    let daemon = PreJitDaemon::new();
    let count = daemon
        .precompile_library(&loaded, &cache_path)
        .expect("Pre-compilation failed");
    assert!(count > 0);

    // 4. Verify cache file was generated and is valid
    assert!(cache_path.exists());

    let mut runtime = LarRuntime::new();
    runtime
        .load_library("libgame_core.so", &elf_bytes)
        .expect("Failed to load library into runtime");
    runtime
        .load_execution_cache(&cache_path)
        .expect("Failed to load generated cache");
    let runtime_nodes = CallgraphAnalyzer::build_startup_graph(&runtime.loaded_libraries[0]);
    let startup = &runtime_nodes[0];
    let mut ctx = Arm64CpuContext::new();
    ctx.pc = startup.address as u64;
    assert_ne!(compiled_base, runtime.loaded_libraries[0].load_base);
    assert!(runtime.jit_engine.has_cached_block(&ctx, &startup.opcodes));

    let _ = std::fs::remove_file(cache_path);
}

#[test]
fn test_baseline_profile_parser() {
    let profile_content = "\
Lcom/unity3d/player/UnityPlayerActivity;
Lcom/example/engine/NativeBridge;->nativeInit()V
Lcom/example/engine/NativeBridge;->renderFrame()V
";
    let summary = BaselineProfileParser::parse(profile_content.as_bytes()).unwrap();
    assert_eq!(summary.startup_classes.len(), 1);
    assert_eq!(summary.hot_method_count, 2);
    assert_eq!(
        summary.startup_classes[0],
        "Lcom/unity3d/player/UnityPlayerActivity;"
    );
}

#[test]
fn startup_discovery_exposes_jni_and_init_contract() {
    let elf_bytes = generate_synthetic_arm64_so("libstartup.so", "JNI_OnLoad");
    let mut runtime = LarRuntime::new();
    runtime.load_library("libstartup.so", &elf_bytes).unwrap();

    assert_eq!(
        runtime.loaded_libraries[0].lifecycle(),
        LibraryLifecycle::Loaded
    );
    let contract = runtime.prepare_phase3_startup()[0].clone();

    assert_eq!(
        runtime.loaded_libraries[0].lifecycle(),
        LibraryLifecycle::StartupPrepared
    );
    assert!(contract.is_ready());
    assert_eq!(contract.init_routines.len(), 1);
    assert!(contract.jni_on_load.is_some());
    assert!(runtime.native_startup_ready());
    assert!(!runtime.is_phase3_ready());
    assert!(!runtime.phase3_readiness().can_start_art());

    let second = runtime.prepare_phase3_startup();
    assert_eq!(second, &[contract]);
    assert_eq!(
        runtime.loaded_libraries[0].lifecycle(),
        LibraryLifecycle::StartupPrepared
    );
}

#[test]
fn unsupported_startup_instruction_is_rejected() {
    let mut block = lar::jit::IrBlock::new(0x1000);
    let inst = lar::jit::Arm64Decoder::decode(0xffffffff);
    assert!(block.translate_arm64_inst_checked(&inst).is_err());
}
