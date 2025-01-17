{
  description = "Flash utility for the Qoob Pro modchip";

  inputs = {
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs: let
    inherit (inputs.nixpkgs) lib;
    defaultSystems = lib.systems.flakeExposed;
    argsForSystem = system: {
      pkgs = import inputs.nixpkgs {
        inherit system;
        overlays = [
          (import inputs.rust-overlay)
        ];
        config = {};
      };
    };
    allArgs = lib.genAttrs defaultSystems argsForSystem;
    eachSystem = fn: lib.genAttrs defaultSystems (system: fn allArgs."${system}");

    rqoobPkg = {
      lib,
      callPackage,
      stdenv,
      pkg-config,
      installShellFiles,
      udev,
    }: let
      naersk = callPackage inputs.naersk {};
    in
      naersk.buildPackage {
        src = ./.;

        nativeBuildInputs = [pkg-config installShellFiles];
        buildInputs = [udev];

        postInstall =
          (lib.optionalString stdenv.isLinux ''
            install -D "$src/70-qoob.rules" "$out/lib/udev/rules.d/70-qoob.rules"
          '')
          + ''
            installShellCompletion --cmd rqoob \
              --bash <("$out/bin/rqoob" gen-completions bash) \
              --fish <("$out/bin/rqoob" gen-completions fish) \
              --zsh <("$out/bin/rqoob" gen-completions zsh)
          '';
      };
  in {
    formatter = eachSystem ({pkgs, ...}:
      pkgs.writeShellScriptBin "formatter" ''
        ${pkgs.alejandra}/bin/alejandra flake.nix
      '');

    devShells = eachSystem ({pkgs, ...}: {
      default = pkgs.mkShell {
        name = "rqoob";
        inputsFrom = [inputs.self.packages."${pkgs.system}".default];
        nativeBuildInputs = [
          (pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "rust-analyzer"
              "rust-src"
            ];
          })
        ];
      };
    });

    packages = eachSystem ({pkgs, ...}: {
      default = pkgs.callPackage rqoobPkg {};
    });

    nixosModules.default = {
      config,
      lib,
      pkgs,
      ...
    }: let
      cfg = config.programs.rqoob;
      rqoob = pkgs.callPackage rqoobPkg {};
    in {
      options = {
        programs.rqoob = {
          enable = lib.mkEnableOption "Enable rqoob, a Qoob flash utility";
        };
      };
      config = lib.mkIf cfg.enable {
        environment.systemPackages = [rqoob];
        services.udev.packages = [rqoob];
      };
    };

    overlays.default = final: prev: {
      rqoob = final.callPackage rqoobPkg {};
    };
  };
}
