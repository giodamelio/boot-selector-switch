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

          virtual-switch-run = pkgs.writeShellApplication {
            name = "virtual-switch-run";
            runtimeInputs = with pkgs; [
              self'.packages.virtual-switch
              linuxPackages.usbip
              netcat-gnu
              kmod
            ];
            text = ''
              # Ensure vhci-hcd module is loaded
              sudo modprobe vhci-hcd

              # Start virtual-switch in background
              virtual-switch &
              VS_PID=$!

              # Cleanup function: detach usbip and kill virtual-switch
              cleanup() {
                  echo "Detaching USB/IP device..."
                  sudo usbip detach -p 0 2>/dev/null || true
                  kill $VS_PID 2>/dev/null || true
              }
              trap cleanup EXIT

              # Poll TCP port 3240 until the server is ready
              echo "Waiting for virtual switch server..."
              while ! nc -z 127.0.0.1 3240 2>/dev/null; do
                  sleep 0.1
              done
              # Attach the virtual device
              sudo usbip attach -r 127.0.0.1 -b 0-0-0

              # Wait for the virtual-switch process (TUI runs in foreground)
              wait $VS_PID
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
              hidrd
              hidapitester
              tinyxxd
            ]);
        };
      };
    };
}
