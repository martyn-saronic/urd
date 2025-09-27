{
  description = "URD Core - IPC-agnostic Universal Robots control library";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        packages = {
          urd-core = pkgs.rustPlatform.buildRustPackage {
            pname = "urd-core";
            version = "0.1.0";
            
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            
            # Pure dependencies - no networking/IPC libraries
            buildInputs = with pkgs; [ 
              openssl 
              pkg-config 
            ];
            
            meta = {
              description = "IPC-agnostic Universal Robots control library";
              longDescription = ''
                Pure robot control library with no transport dependencies.
                Provides URScript execution, command queuing, RTDE protocol
                implementation, and robot state management.
              '';
            };
          };
          
          default = self.packages.${system}.urd-core;
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
        };
        
        # Library can be used by other flakes
        lib = {
          # Expose the package for other flakes to use
          urdCore = self.packages.${system}.urd-core;
        };
      });
}