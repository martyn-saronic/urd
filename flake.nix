{
  description = "UR10e scripting framework development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain
            cargo
            rustc
            rustfmt
            rust-analyzer
            clippy
            
            # Docker tools
            docker
            docker-compose
          ];
          
          shellHook = ''
            echo "🤖 UR10e Development Environment"
            echo "Rust: $(rustc --version)"
            echo "Docker: $(docker --version)"
            echo "Available: Rust toolchain, Docker"
            echo ""
            echo "Commands:"
            echo "  start-sim      - Start UR10e simulator"
            echo "  stop-sim       - Stop UR10e simulator"
            echo "  ur-init        - Power on and initialize UR robot"
            echo "  urd            - Universal Robots daemon - command interpreter (Rust)"
            echo "  cargo build    - Build Rust workspace"
            echo ""
            
            # Create shell aliases for convenience
            # Get the repository root directory
            REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
            
            alias start-sim="$REPO_ROOT/scripts/start-sim.sh"
            alias stop-sim="$REPO_ROOT/scripts/stop-sim.sh"
            alias ur-init="$REPO_ROOT/scripts/ur-init.sh"
            alias urd="$REPO_ROOT/target/release/urd || (cd $REPO_ROOT && cargo build --release --bin urd && $REPO_ROOT/target/release/urd)"
          '';
        };
      }
    );
}