{
  description = "Rust development environment for MinecraftAnalysis";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.crane.url = "github:ipetkov/crane";

  outputs = { self, nixpkgs, crane }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          rustToolchain = pkgs.rust-bin.stable.latest.default or null;
          # nixpkgs exposes the stable components directly; keep one package list
          # so the shell and checks use the same locked toolchain.
          rustPackages = if rustToolchain == null then [
            pkgs.rustc
            pkgs.cargo
            pkgs.rustfmt
            pkgs.clippy
          ] else [ rustToolchain ];
        in {
          default = pkgs.mkShell {
            packages = rustPackages ++ [
              pkgs.pkg-config
              pkgs.git
            ];
            RUST_BACKTRACE = "1";
          };
        });

      checks = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          craneLib = crane.mkLib pkgs;
          src = craneLib.cleanCargoSource self;
          commonArgs = {
            inherit src;
            pname = "minecraft-analysis";
            version = "0.1.0";
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in {
          format = craneLib.cargoFmt { inherit src; };
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--workspace --all-targets -- -D warnings";
          });
          test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
            cargoTestExtraArgs = "--workspace";
          });
        });
    };
}
