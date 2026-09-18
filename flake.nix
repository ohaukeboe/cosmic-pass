{
  description = "Quick-access popup for Proton Pass on the COSMIC desktop";

  # The channel tarball is smaller and faster to fetch than the GitHub archive, and it always
  # points at a nixpkgs revision that passed the channel's tests.
  inputs.nixpkgs.url = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.zst";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      # `pkgs.system` is deprecated in favour of this.
      systemOf = pkgs: pkgs.stdenv.hostPlatform.system;

      # Loaded at runtime by winit/wgpu through dlopen, so they must be on LD_LIBRARY_PATH
      # rather than only linked at build time.
      runtimeLibs =
        pkgs: with pkgs; [
          wayland
          libxkbcommon
          vulkan-loader
          libGL
          fontconfig
          freetype
          expat
        ];
    in
    {
      packages = forAllSystems (pkgs: {
        default = self.packages.${systemOf pkgs}.cosmic-pass;

        cosmic-pass = pkgs.rustPlatform.buildRustPackage {
          pname = "cosmic-pass";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./src
              ./tests
              ./data
            ];
          };

          cargoLock = {
            lockFile = ./Cargo.lock;
            # libcosmic vendors iced as a git submodule, which the hash-pinned fetcher does not
            # fetch. This uses builtins.fetchGit, which does, at the revisions Cargo.lock pins.
            allowBuiltinFetchGit = true;
          };

          nativeBuildInputs = with pkgs; [
            pkg-config
            makeWrapper
          ];

          buildInputs = runtimeLibs pkgs;

          # The clipboard tests spawn helper processes and wait on real timeouts; the parser
          # tests read fixtures. All of it runs without a display.
          checkFlags = [
            # Needs a Wayland compositor.
            "--skip=app::surface"
          ];

          postInstall = ''
            install -Dm644 data/io.github.ohaukeboe.CosmicPass.desktop \
              $out/share/applications/io.github.ohaukeboe.CosmicPass.desktop
            install -Dm644 data/io.github.ohaukeboe.CosmicPass.metainfo.xml \
              $out/share/metainfo/io.github.ohaukeboe.CosmicPass.metainfo.xml
            install -Dm644 data/icons/io.github.ohaukeboe.CosmicPass.svg \
              $out/share/icons/hicolor/scalable/apps/io.github.ohaukeboe.CosmicPass.svg

            # The shipped unit points at ~/.local/bin; a flake install runs from the store.
            install -Dm644 data/cosmic-pass.service \
              $out/share/systemd/user/cosmic-pass.service
            substituteInPlace $out/share/systemd/user/cosmic-pass.service \
              --replace-fail '%h/.local/bin/cosmic-pass' "$out/bin/cosmic-pass"

            wrapProgram $out/bin/cosmic-pass \
              --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (runtimeLibs pkgs)}"
          '';

          meta = {
            description = "Quick-access popup for Proton Pass on the COSMIC desktop";
            longDescription = ''
              A keyboard-driven popup that searches Proton Pass items and copies passwords,
              usernames and one-time codes to the clipboard. Unofficial; it drives the official
              pass-cli tool, which must be installed and signed in separately.
            '';
            homepage = "https://github.com/ohaukeboe/cosmic-pass";
            license = pkgs.lib.licenses.mit;
            mainProgram = "cosmic-pass";
            platforms = pkgs.lib.platforms.linux;
          };
        };
      });

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = pkgs.lib.getExe self.packages.${systemOf pkgs}.cosmic-pass;
        };
      });

      overlays.default = _final: prev: {
        cosmic-pass = self.packages.${prev.stdenv.hostPlatform.system}.cosmic-pass;
      };

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages =
            (with pkgs; [
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
            ])
            ++ runtimeLibs pkgs;

          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          # cargo-llvm-cov needs llvm tools matching rustc's LLVM version.
          LLVM_COV = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-cov";
          LLVM_PROFDATA = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-profdata";

          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath (runtimeLibs pkgs)}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
          '';
        };
      });

      checks = forAllSystems (pkgs: {
        package = self.packages.${systemOf pkgs}.cosmic-pass;
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);
    };
}
