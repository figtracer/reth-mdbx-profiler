//! Build script for compiling BPF programs

fn main() {
    #[cfg(target_os = "linux")]
    {
        use libbpf_cargo::SkeletonBuilder;
        use std::path::PathBuf;

        let src = PathBuf::from("bpf/mdbx_tracer.bpf.c");
        let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("mdbx_tracer.bpf.o");

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

        println!("cargo:rerun-if-changed=bpf/mdbx_tracer.bpf.c");
        println!("cargo:rerun-if-changed=bpf/vmlinux.h");
    }

    #[cfg(not(target_os = "linux"))]
    {
        panic!("This profiler requires Linux with eBPF support. Please build and run on a Linux machine.");
    }
}
