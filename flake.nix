{
  description = "Quick-access popup for Proton Pass on the COSMIC desktop";

  # The channel tarball is smaller and faster to fetch than the GitHub archive, and it always
  # points at a nixpkgs revision that passed the channel's tests.
  inputs.nixpkgs.url = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.zst";

  # `pass-cli` alone, pinned to one immutable release rather than the rolling channel, and
  # kept as a separate input so that it survives an `inputs.nixpkgs.follows` a consumer adds.
  # It has to: `pass-cli` publishes no stability policy and changes its command surface in
  # patch releases, so the version this app drives is part of what was tested, not a detail
  # to inherit from whoever installs it. A release channel lags far enough to matter —
  # nixos-26.05 ships pass-cli 2.0.2 and nixos-25.11 does not package it at all, against the
  # 2.3 this app needs to address a field inside a section.
  #
  # The cost is one extra nixpkgs fetch and evaluation for consumers. Consumers who would
  # rather pay nothing, or who want a different `pass-cli`, override it:
  #   cosmic-pass.packages.<system>.default.override { proton-pass-cli = pkgs.proton-pass-cli; }
  inputs.nixpkgs-pass-cli.url = "https://releases.nixos.org/nixos/unstable/nixos-26.11pre1075591.e554fab72f81/nixexprs.tar.zst";

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-pass-cli,
    }:
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

        # `makeOverridable`, so `proton-pass-cli` is a package argument a consumer can
        # replace: `.override { proton-pass-cli = ...; }`. `callPackage` would defeat the
        # point — it fills the argument from the evaluating package set, which is exactly the
        # nixpkgs a `follows` replaces.
        cosmic-pass = pkgs.lib.makeOverridable (
          {
            proton-pass-cli,
          }:
          pkgs.rustPlatform.buildRustPackage {
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

              # lib/systemd/user is where NixOS's systemd.packages looks for user units.
              # The shipped unit points at ~/.local/bin; a flake install runs from the store.
              install -Dm644 data/cosmic-pass.service \
                $out/lib/systemd/user/cosmic-pass.service
              substituteInPlace $out/lib/systemd/user/cosmic-pass.service \
                --replace-fail '%h/.local/bin/cosmic-pass' "$out/bin/cosmic-pass"

              # `--suffix`, not `--prefix`: a `pass-cli` the user already has on PATH still wins,
              # so this only supplies one when the environment offers none. A NixOS user unit is
              # exactly that case — `systemd.user.services.<name>.path` replaces the inherited
              # PATH with a minimal one, which left the resident process reporting the tool as
              # missing however the user had installed it.
              wrapProgram $out/bin/cosmic-pass \
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (runtimeLibs pkgs)}" \
                --suffix PATH : "${pkgs.lib.makeBinPath [ proton-pass-cli ]}"
            '';

            meta = {
              description = "Quick-access popup for Proton Pass on the COSMIC desktop";
              longDescription = ''
                A keyboard-driven popup that searches Proton Pass items and copies passwords,
                usernames and one-time codes to the clipboard. Unofficial; it drives the official
                pass-cli tool, which ships with this package and must be signed in separately.
              '';
              homepage = "https://github.com/ohaukeboe/cosmic-pass";
              license = pkgs.lib.licenses.mit;
              mainProgram = "cosmic-pass";
              platforms = pkgs.lib.platforms.linux;
            };
          }
        ) { proton-pass-cli = nixpkgs-pass-cli.legacyPackages.${systemOf pkgs}.proton-pass-cli; };
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
            ++ [
              # The same pinned build the package wraps, so `tests/pass_cli_contract.rs`
              # exercises the `pass-cli` this project actually ships against. `pkgs`'s own
              # would be a different version -- see the note on the input above.
              nixpkgs-pass-cli.legacyPackages.${systemOf pkgs}.proton-pass-cli
            ]
            ++ runtimeLibs pkgs;

          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          # Inside this shell `pass-cli` is guaranteed, so its absence is a broken flake rather
          # than a contributor without Nix: fail the contract suite instead of skipping it.
          COSMIC_PASS_REQUIRE_CLI = "1";
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
