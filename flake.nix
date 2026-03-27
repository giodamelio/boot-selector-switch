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

      flake = {
        nixosModules.default = ./nixos-module;
        nixosModules.boot-selector-switch = ./nixos-module;
      };

      perSystem = {
        self',
        config,
        pkgs,
        ...
      }: let
        buildEfiShim = extraArgs:
          config.rust-project.crane-lib.buildPackage {
            inherit (config.rust-project) src;
            pname = "boot-selector-switch-efi-shim";
            cargoExtraArgs = "-p boot-selector-switch-efi-shim --target x86_64-unknown-uefi" + extraArgs;
            cargoArtifacts = null;
            doCheck = false;
            strictDeps = true;
            installPhaseCommand = ''
              mkdir -p $out
              cp target/x86_64-unknown-uefi/release/boot-selector-switch-efi-shim.efi $out/
            '';
          };

        buildTestEntry = name: text:
          config.rust-project.crane-lib.buildPackage {
            inherit (config.rust-project) src;
            pname = "test-entry-${name}";
            cargoExtraArgs = "-p test-efi-stub --target x86_64-unknown-uefi --features test-efi-stub/qemu";
            cargoArtifacts = null;
            doCheck = false;
            strictDeps = true;
            TEST_ENTRY_TEXT = text;
            installPhaseCommand = ''
              mkdir -p $out
              cp target/x86_64-unknown-uefi/release/test-efi-stub.efi $out/${name}.efi
            '';
          };

        testVmNixos = inputs.nixpkgs.lib.nixosSystem {
          system = "x86_64-linux";
          specialArgs = {
            boot-selector-switch-packages = self'.packages;
          };
          modules = [
            "${inputs.nixpkgs}/nixos/modules/virtualisation/qemu-vm.nix"
            ./nixos-module
            ./nixos-module/test-vm.nix
          ];
        };
      in {
        rust-project.crateNixFile = "crate.nix";

        treefmt = {
          projectRootFile = "flake.nix";
          programs.rustfmt.enable = true;
          programs.alejandra.enable = true;
          programs.statix.enable = true;
        };

        packages = {
          # Production efi-shim (no qemu feature)
          efi-shim = buildEfiShim "";

          # QEMU-compatible efi-shim (enables uefi/qemu)
          efi-shim-qemu = buildEfiShim " --features boot-selector-switch-efi-shim/qemu";

          # Test entry stub for Windows position
          test-entry-windows = buildTestEntry "windows" "Windows";

          # Test VM — always rebuilds the disk overlay to avoid stale image issues.
          # EFI vars are preserved in .vm-state/ so debug mode persists across reboots.
          test-vm = pkgs.writeShellApplication {
            name = "test-vm";
            text = ''
              mkdir -p .vm-state
              rm -f .vm-state/boot-selector-test.qcow2
              export NIX_DISK_IMAGE=.vm-state/boot-selector-test.qcow2
              export NIX_EFI_VARS=.vm-state/boot-selector-test-efi-vars.fd
              exec ${testVmNixos.config.system.build.vm}/bin/run-boot-selector-test-vm "$@"
            '';
          };

          test-vm-usb = pkgs.writeShellApplication {
            name = "test-vm-usb";
            text = ''
              sudo ${self'.packages.test-vm}/bin/test-vm \
                -device usb-host,bus=xhci.0,vendorid=0x6666,productid=0xB007
            '';
          };

          pico-firmware = config.rust-project.crane-lib.buildPackage {
            src = pkgs.lib.cleanSourceWith {
              src = ./.;
              filter = path: type:
                (pkgs.lib.hasSuffix ".x" path)
                || (config.rust-project.crane-lib.filterCargoSources path type);
            };
            pname = "pico-firmware";
            cargoExtraArgs = "-p pico-firmware --target thumbv8m.main-none-eabihf";
            DEFMT_LOG = "trace";
            cargoArtifacts = null;
            doCheck = false;
            strictDeps = true;
            installPhaseCommand = ''
              mkdir -p $out
              cp target/thumbv8m.main-none-eabihf/release/pico-firmware $out/pico-firmware.elf
            '';
          };

          flash-pico-firmware = pkgs.writeShellApplication {
            name = "flash-pico-firmware";
            runtimeInputs = [pkgs.probe-rs-tools];
            text = ''
              probe-rs run --chip RP235x ${self'.packages.pico-firmware}/pico-firmware.elf
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
              probe-rs-tools
              elf2uf2-rs
              picotool
            ]);
        };
      };
    };
}
