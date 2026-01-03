//! Build script for compiling BPF programs

fn main() {
    #[cfg(target_os = "linux")]
    {
        use libbpf_cargo::SkeletonBuilder;
        use std::path::PathBuf;

        let src = PathBuf::from("bpf/mdbx_tracer.bpf.c");
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let out = out_dir.join("mdbx_tracer.bpf.o");

        // Check if vmlinux.h exists
        let vmlinux = PathBuf::from("bpf/vmlinux.h");
        if !vmlinux.exists() {
            println!(
                "cargo:warning=vmlinux.h not found - run scripts/setup-node.sh to generate it"
            );
            println!("cargo:warning=Skipping BPF compilation");
            return;
        }

        SkeletonBuilder::new()
            .source(&src)
            .clang_args(["-I", "bpf/", "-Wno-compare-distinct-pointer-types"])
            .build_and_generate(&out)
            .expect("Failed to build BPF program");

        // Copy the BPF object file to the target directory so it can be found at runtime.
        // The binary looks for mdbx_tracer.bpf.o in the same directory as the executable.
        // OUT_DIR is something like: target/release/build/<pkg>-<hash>/out
        // We need to copy to: target/release/
        if let Some(target_dir) = out_dir
            .ancestors()
            .find(|p| p.ends_with("release") || p.ends_with("debug"))
        {
            let dest = target_dir.join("mdbx_tracer.bpf.o");
            if let Err(e) = std::fs::copy(&out, &dest) {
                println!(
                    "cargo:warning=Failed to copy BPF object to target dir: {}",
                    e
                );
            } else {
                println!("cargo:warning=Copied BPF object to {:?}", dest);
            }
        }

        println!("cargo:rerun-if-changed=bpf/mdbx_tracer.bpf.c");
        println!("cargo:rerun-if-changed=bpf/vmlinux.h");
    }

    #[cfg(not(target_os = "linux"))]
    {
        panic!("This profiler requires Linux with eBPF support. Please build and run on a Linux machine.");
    }
}
