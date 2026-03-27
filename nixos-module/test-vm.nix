{
  boot-selector-switch-packages,
  lib,
  pkgs,
  ...
}: let
  packages = boot-selector-switch-packages;
in {
  system.stateVersion = "25.11";
  networking.hostName = "boot-selector-test";

  # No networking needed
  networking.useDHCP = false;

  # Bootloader — timeout 0 so systemd-boot boots immediately when LoaderEntryOneShot is set
  boot.loader.systemd-boot.enable = true;
  boot.loader.systemd-boot.netbootxyz.enable = true;
  boot.loader.timeout = 0;

  # Boot selector switch
  boot.loader.boot-selector-switch = {
    enable = true;
    package = packages.efi-shim-qemu;
    positionMap = {
      "1" = "nixos-latest.conf";
      "2" = "windows.conf";
      "3" = "netbootxyz.conf";
    };
  };

  # Windows test stub (prints "Windows" and shuts down)
  boot.loader.systemd-boot.extraFiles = {
    "EFI/test/windows.efi" = "${packages.test-entry-windows}/windows.efi";
  };
  boot.loader.systemd-boot.extraEntries = {
    "windows.conf" = ''
      title Windows
      efi /EFI/test/windows.efi
    '';
  };

  # QEMU VM settings
  virtualisation = {
    useBootLoader = true;
    useEFIBoot = true;
    memorySize = 512;
    qemu.options = [
      "-nographic"
      "-device qemu-xhci,id=xhci"
    ];
  };

  # Serial console for -nographic
  boot.kernelParams = ["console=ttyS0,115200"];

  # Auto-login root for easy testing
  services.getty.autologinUser = "root";

  # Minimal tools for debugging
  environment.systemPackages = with pkgs; [
    efibootmgr
    efivar
  ];

  # Suppress warning about missing hardware config
  hardware.enableRedistributableFirmware = lib.mkForce false;
}
