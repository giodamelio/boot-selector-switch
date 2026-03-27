{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.boot.loader.boot-selector-switch;
  espMountPoint = config.boot.loader.efi.efiSysMountPoint;

  bootEntryLabel = "Boot Selector Switch";

  # Script to create nixos-latest.conf and register the efi-shim in UEFI boot order.
  # Called from extraInstallCommands after systemd-boot has installed.
  installScript = pkgs.writeShellApplication {
    name = "boot-selector-switch-install";
    runtimeInputs = with pkgs; [coreutils gnused gnugrep findutils efibootmgr util-linux];
    excludeShellChecks = ["SC2162"];
    text = ''
      ESP="${espMountPoint}"

      # --- Create nixos-latest.conf pointing to the latest NixOS generation ---
      latest=$(find "$ESP/loader/entries" -maxdepth 1 -name 'nixos-generation-*.conf' 2>/dev/null | sort -V | tail -1)
      if [ -n "$latest" ]; then
        cp "$latest" "$ESP/loader/entries/nixos-latest.conf"
        sed -i 's/^title .*/title NixOS (Latest)/' "$ESP/loader/entries/nixos-latest.conf"
      fi

      # --- Register efi-shim in UEFI boot order ---
      # Only attempt if EFI variables are accessible and efibootmgr works.
      # During VM image builds, EFI runtime may not be available.
      if ! efibootmgr > /dev/null 2>&1; then
        echo "boot-selector-switch: efibootmgr not functional, skipping boot entry registration"
        exit 0
      fi

      # Find the disk and partition number for our ESP
      ESP_DEV=$(findmnt --noheadings --output SOURCE "$ESP")
      DISK="/dev/$(lsblk -ndo PKNAME "$ESP_DEV")"
      PART=$(cat "/sys/class/block/$(basename "$ESP_DEV")/partition")

      # Remove any existing entries with our label (and only our label)
      efibootmgr | grep "${bootEntryLabel}" | grep -oP 'Boot\K[0-9A-Fa-f]{4}' | while read bootnum; do
        efibootmgr --quiet --bootnum "$bootnum" --delete-bootnum
      done || true

      # Create new boot entry at the front of the boot order, targeting our ESP only
      efibootmgr --quiet --create \
        --disk "$DISK" --part "$PART" \
        --loader '\EFI\boot-selector-switch\boot-selector-switch.efi' \
        --label '${bootEntryLabel}'

      echo "boot-selector-switch: registered in UEFI boot order on $ESP_DEV"
    '';
  };
in {
  options.boot.loader.boot-selector-switch = {
    enable = lib.mkEnableOption "boot selector switch EFI shim";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The efi-shim package to install on the ESP.";
    };

    positionMap = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = {
        "1" = "nixos-latest.conf";
        "2" = "windows.conf";
        "3" = "netbootxyz.conf";
        "6" = "DEBUG";
      };
      description = ''
        Mapping from switch position (1-6) to systemd-boot entry filename.
        The special value "DEBUG" marks the debug toggle position.

        Currently for documentation only — the mapping is compiled into
        the efi-shim binary. Ensure these match the Rust source.
        A future version will read this from a config file on the ESP.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = config.boot.loader.systemd-boot.enable;
        message = "boot-selector-switch requires systemd-boot (boot.loader.systemd-boot.enable = true)";
      }
    ];

    boot.loader.efi.canTouchEfiVariables = true;

    # Install the efi-shim to its own directory on the ESP.
    boot.loader.systemd-boot.extraFiles = {
      "EFI/boot-selector-switch/boot-selector-switch.efi" = "${cfg.package}/boot-selector-switch-efi-shim.efi";
    };

    boot.loader.systemd-boot.extraInstallCommands = ''
      ${installScript}/bin/boot-selector-switch-install
    '';
  };
}
