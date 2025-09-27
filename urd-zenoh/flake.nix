{
  description = "URD Zenoh - Zenoh transport wrapper for URD Core";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    
    # Import urd-core from the sibling directory
    urd-core = {
      url = "path:../urd-core";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, urd-core }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        
        # Get urd-core from input
        urd-core-lib = urd-core.packages.${system}.default;
        
      in {
        packages = {
          urd-zenoh = pkgs.rustPlatform.buildRustPackage {
            pname = "urd-zenoh";
            version = "0.1.0";
            
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            
            # Dependencies including urd-core
            buildInputs = with pkgs; [ 
              openssl 
              pkg-config
            ];
            
            # Make urd-core available during build
            CARGO_TARGET_DIR = "target";
            
            meta = {
              description = "Zenoh transport wrapper for URD Core";
              longDescription = ''
                Provides Zenoh RPC transport layer for URD robot control.
                Demonstrates how to wrap urd-core with a specific transport.
              '';
            };
          };
          
          # Also build the daemon binary
          urd = self.packages.${system}.urd-zenoh;
          
          default = self.packages.${system}.urd-zenoh;
        };
        
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustc
            cargo
            rust-analyzer
            clippy
            rustfmt
            
            # Development tools
            just
            bacon
          ];
          
          RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
          
          # Make urd-core available during development
          shellHook = ''
            echo "URD Zenoh development environment"
            echo "urd-core available as dependency"
          '';
        };
        
        # Convenience apps
        apps = {
          urd = flake-utils.lib.mkApp {
            drv = self.packages.${system}.urd;
            exePath = "/bin/urd";
          };
          
          urd-cli = flake-utils.lib.mkApp {
            drv = self.packages.${system}.urd-zenoh;
            exePath = "/bin/urd_cli";
          };
          
          default = self.apps.${system}.urd;
        };
      });
}