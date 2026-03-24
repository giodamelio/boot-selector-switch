{
  description = "Boot selector switch - physical OS selector for systemd-boot";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-flake.url = "github:juspay/rust-flake";
    treefmt-nix.url = "github:numtide/treefmt-nix";
  };

  outputs = inputs:
    inputs.flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux"];

      imports = [
        inputs.rust-flake.flakeModules.default
        inputs.rust-flake.flakeModules.nixpkgs
        inputs.treefmt-nix.flakeModule
      ];

      perSystem = {
        self',
        config,
        pkgs,
        ...
      }: {
        rust-project.crateNixFile = "crate.nix";

        treefmt = {
          projectRootFile = "flake.nix";
          programs.rustfmt.enable = true;
          programs.alejandra.enable = true;
          programs.statix.enable = true;
        };

        packages = {
          efi-shim = pkgs.stdenv.mkDerivation {
            pname = "efi-shim";
            version = "0.1.0";
            inherit (config.rust-project) src;
            nativeBuildInputs = [config.rust-project.toolchain];
            buildPhase = ''
              cargo build --target x86_64-unknown-uefi -p efi-shim --release
            '';
            installPhase = ''
              mkdir -p $out
              cp target/x86_64-unknown-uefi/release/efi-shim.efi $out/
            '';
          };
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [self'.devShells.rust];
          packages =
            [
              config.treefmt.build.wrapper
            ]
            ++ (with pkgs; [
              linuxPackages.usbip
              usbutils
              qemu
              OVMF
              dosfstools
              mtools
              efibootmgr
            ]);
        };
      };
    };
}
