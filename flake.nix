{
  description = "Flash utility for the Qoob Pro modchip";

  inputs = {
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    systems.flake = false;
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    naersk = {
      url = "github:semnix/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { ... } @ inputs: inputs.flake-parts.lib.mkFlake {
    inherit inputs;
  } ({ config, flake-parts-lib, getSystem, inputs, lib, options, ... }:
    let
      rootConfig = config;
      rootOptions = options;
    in
    {
      _file = ./flake.nix;
      imports = [ ];
      config.perSystem = { config, inputs', nixpkgs, options, pkgs, system, ... }:
        let
          systemConfig = config;
          systemOptions = options;

          naersk' = pkgs.callPackage inputs.naersk { };
        in
        {
          _file = ./flake.nix;
          config = {
            _module.args.pkgs = import inputs.nixpkgs {
              inherit system;
              overlays = [
                (import inputs.rust-overlay)
              ];
              config = { };
            };

            devShells.default = pkgs.callPackage
              ({ mkShell
              , pkg-config
              , rust-bin
              , udev
              }: mkShell {
                name = "rqoob";
                nativeBuildInputs = [
                  pkg-config
                  (rust-bin.stable.latest.default.override {
                    extensions = [
                      "rust-analyzer"
                      "rust-src"
                    ];
                  })
                ];

                buildInputs = [
                  udev
                ];
              })
              { };

            packages.default = pkgs.callPackage
              ({ lib
              , stdenv
              , pkg-config
              , installShellFiles
              , udev
              }: naersk'.buildPackage {
                src = ./.;

                nativeBuildInputs = [ pkg-config installShellFiles ];
                buildInputs = [ udev ];

                postInstall = (lib.optionalString stdenv.isLinux ''
                  install -D "$src/70-qoob.rules" "$out/lib/udev/rules.d/70-qoob.rules"
                '') + ''
                  installShellCompletion --cmd rqoob \
                    --bash <("$out/bin/rqoob" gen-completions bash) \
                    --fish <("$out/bin/rqoob" gen-completions fish) \
                    --zsh <("$out/bin/rqoob" gen-completions zsh)
                '';
              })
              { };
          };
        };
      config.systems = import inputs.systems;
      config.flake.nixosModules.default = { config, lib, pkgs, ... }: let
        cfg = config.programs.rqoob;
        rqoob = inputs.self.packages.${pkgs.system}.default;
      in {
        options = {
          programs.rqoob = {
            enable = lib.mkEnableOption "Enable rqoob, a Qoob flash utility";
          };
        };
        config = lib.mkIf cfg.enable {
          environment.systemPackages = [ rqoob ];
          services.udev.packages = [ rqoob ];
        };
      };
  });
}
