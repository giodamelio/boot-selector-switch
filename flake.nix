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
          efi-shim = config.rust-project.crane-lib.buildPackage {
            inherit (config.rust-project) src;
            pname = "boot-selector-switch-efi-shim";
            # Build only the efi-shim crate, targeting UEFI instead of the host platform.
            cargoExtraArgs = "-p boot-selector-switch-efi-shim --target x86_64-unknown-uefi --features boot-selector-switch-efi-shim/qemu";
            # Skip crane's buildDepsOnly phase. Crane generates a dummy main.rs to
            # pre-compile dependencies, but the UEFI linker expects an `efi_main`
            # entry point (provided by the #[entry] macro), so the dummy source
            # fails to link.
            cargoArtifacts = null;
            # No test runner exists for x86_64-unknown-uefi.
            doCheck = false;
            strictDeps = true;
            # Crane's default install phase looks for binaries in target/<host>/release,
            # but our output is under the UEFI target triple directory.
            installPhaseCommand = ''
              mkdir -p $out
              cp target/x86_64-unknown-uefi/release/boot-selector-switch-efi-shim.efi $out/
            '';
          };

          esp-image = pkgs.stdenv.mkDerivation {
            pname = "esp-image";
            version = "0.1.0";
            nativeBuildInputs = with pkgs; [dosfstools mtools];
            buildCommand = ''
              # Build a 64MB FAT32 ESP image
              dd if=/dev/zero of=esp.img bs=1M count=64
              mkfs.fat -F 32 esp.img
              mmd -i esp.img ::/EFI
              mmd -i esp.img ::/EFI/BOOT
              mcopy -i esp.img ${self'.packages.efi-shim}/boot-selector-switch-efi-shim.efi ::/EFI/BOOT/BOOTX64.EFI
              mkdir -p $out
              cp esp.img $out/
            '';
          };

          test-vm = pkgs.writeShellApplication {
            name = "test-vm";
            runtimeInputs = [pkgs.qemu];
            text = ''
              # Copy OVMF_VARS.fd to a writable location (UEFI needs to write variables)
              TMPDIR=$(mktemp -d)
              cleanup() { rm -rf "$TMPDIR"; }
              trap cleanup EXIT
              OVMF_VARS="$TMPDIR/OVMF_VARS.fd"
              cp ${pkgs.OVMF.fd}/FV/OVMF_VARS.fd "$OVMF_VARS"
              chmod u+w "$OVMF_VARS"

              ESP="$TMPDIR/esp.img"
              cp ${self'.packages.esp-image}/esp.img "$ESP"
              chmod u+w "$ESP"

              # Launch QEMU with OVMF firmware and pre-built ESP image
              qemu-system-x86_64 \
                -drive if=pflash,format=raw,readonly=on,file=${pkgs.OVMF.fd}/FV/OVMF_CODE.fd \
                -drive if=pflash,format=raw,file="$OVMF_VARS" \
                -drive format=raw,file="$ESP" \
                -nographic \
                -net none
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
