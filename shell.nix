{
  pkgs ? import <nixpkgs> { },
}:
let
  # Shared libraries loaded at runtime by winit/wgpu (dlopen), so they must be on LD_LIBRARY_PATH.
  runtimeLibs = with pkgs; [
    wayland
    libxkbcommon
    vulkan-loader
    libGL
    fontconfig
    freetype
    expat
  ];
  # cargo-llvm-cov needs llvm tools that match rustc's LLVM version.
  llvm = pkgs.rustc.llvmPackages.llvm;
in
pkgs.mkShell {
  packages = with pkgs; [
    bashInteractive
    rustc
    cargo
    clippy
    rustfmt
    rust-analyzer
    pkg-config
    just
    cargo-nextest
    cargo-llvm-cov
    jq
    wl-clipboard
    mesa
  ];

  buildInputs = runtimeLibs;

  RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
  LLVM_COV = "${llvm}/bin/llvm-cov";
  LLVM_PROFDATA = "${llvm}/bin/llvm-profdata";

  shellHook = ''
    export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  '';
}
