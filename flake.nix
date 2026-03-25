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

        packages = let
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
          testEntryNixos = buildTestEntry "nixos" "NixOS (Current)";
          testEntryWindows = buildTestEntry "windows" "Windows";
          testEntryFedora = buildTestEntry "fedora" "Fedora";
        in {
          efi-shim = config.rust-project.crane-lib.buildPackage {
            inherit (config.rust-project) src;
            pname = "boot-selector-switch-efi-shim";
            cargoExtraArgs = "-p boot-selector-switch-efi-shim --target x86_64-unknown-uefi --features boot-selector-switch-efi-shim/qemu";
            # Skip crane's buildDepsOnly phase — the UEFI linker expects an `efi_main`
            # entry point (provided by the #[entry] macro), so the dummy source fails to link.
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

          test-esp = let
            loaderConf = pkgs.writeText "loader.conf" ''
              timeout 5
              default nixos.conf
            '';
            entryConf = name:
              pkgs.writeText "${name}.conf" ''
                title ${name}
                efi /EFI/test/${name}.efi
              '';
          in
            pkgs.runCommand "test-esp" {
              nativeBuildInputs = [pkgs.dosfstools pkgs.mtools];
            } ''
              img="$out/esp.img"
              mkdir -p $out
              mkfs.fat -C "$img" 65536

              mmd -i "$img" ::/EFI
              mmd -i "$img" ::/EFI/BOOT
              mmd -i "$img" ::/EFI/systemd
              mmd -i "$img" ::/EFI/test
              mmd -i "$img" ::/loader
              mmd -i "$img" ::/loader/entries

              mcopy -i "$img" ${self'.packages.efi-shim}/boot-selector-switch-efi-shim.efi ::/EFI/BOOT/BOOTX64.EFI
              mcopy -i "$img" ${pkgs.systemd}/lib/systemd/boot/efi/systemd-bootx64.efi ::/EFI/systemd/systemd-bootx64.efi

              mcopy -i "$img" ${testEntryNixos}/nixos.efi ::/EFI/test/nixos.efi
              mcopy -i "$img" ${testEntryWindows}/windows.efi ::/EFI/test/windows.efi
              mcopy -i "$img" ${testEntryFedora}/fedora.efi ::/EFI/test/fedora.efi

              mcopy -i "$img" ${loaderConf} ::/loader/loader.conf
              mcopy -i "$img" ${entryConf "nixos"} ::/loader/entries/nixos.conf
              mcopy -i "$img" ${entryConf "windows"} ::/loader/entries/windows.conf
              mcopy -i "$img" ${entryConf "fedora"} ::/loader/entries/fedora.conf
            '';

          test-vm = pkgs.writeShellApplication {
            name = "test-vm";
            runtimeInputs = [pkgs.qemu];
            text = ''
              # Persistent state directory for OVMF_VARS (EFI variables survive across runs)
              STATE_DIR=".vm-state"
              mkdir -p "$STATE_DIR"

              OVMF_VARS="$STATE_DIR/OVMF_VARS.fd"
              if [ ! -f "$OVMF_VARS" ]; then
                cp ${pkgs.OVMF.fd}/FV/OVMF_VARS.fd "$OVMF_VARS"
                chmod u+w "$OVMF_VARS"
              fi

              # Copy ESP image to a temp location (rebuilt by Nix each time)
              TMPDIR=$(mktemp -d)
              cleanup() { rm -rf "$TMPDIR"; }
              trap cleanup EXIT

              ESP="$TMPDIR/esp.img"
              cp ${self'.packages.test-esp}/esp.img "$ESP"
              chmod u+w "$ESP"

              # Launch QEMU with OVMF firmware and ESP image
              # Extra arguments can be passed through (e.g. USB device passthrough)
              qemu-system-x86_64 \
                -drive if=pflash,format=raw,readonly=on,file=${pkgs.OVMF.fd}/FV/OVMF_CODE.fd \
                -drive if=pflash,format=raw,file="$OVMF_VARS" \
                -drive format=raw,file="$ESP" \
                -device qemu-xhci,id=xhci \
                -nographic \
                -m 512 \
                -net none \
                "$@"
            '';
          };

          test-vm-usb = pkgs.writeShellApplication {
            name = "test-vm-usb";
            runtimeInputs = [];
            text = ''
              sudo ${self'.packages.test-vm}/bin/test-vm \
                -device usb-host,bus=xhci.0,vendorid=0x6666,productid=0xB007
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
