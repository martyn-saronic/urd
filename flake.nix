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
            echo "  urd-z          - URD with Zenoh integration"
            echo "  urd-zsub       - Zenoh subscriber (usage: urd-zsub [pose|state] or no args for both)"
            echo "  urd-command    - General command client (usage: urd-command <SUBCOMMAND> [OPTIONS])"
            echo "  cargo build    - Build Rust workspace"
            echo ""
            
            # Create shell aliases for convenience
            # Get the repository root directory
            REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
            
            # Set default config path environment variable (if not already set)
            export DEFAULT_CONFIG_PATH=''${DEFAULT_CONFIG_PATH:-"$REPO_ROOT/config/default_config.yaml"}
            
            alias start-sim="$REPO_ROOT/scripts/start-sim.sh"
            alias stop-sim="$REPO_ROOT/scripts/stop-sim.sh"
            alias ur-init="$REPO_ROOT/scripts/ur-init.sh"
            
            # Function to handle urd with arguments
            urd() {
              if [ -f "$REPO_ROOT/target/release/urd" ]; then
                "$REPO_ROOT/target/release/urd" "$@"
              else
                (cd "$REPO_ROOT" && cargo build --release --bin urd && "$REPO_ROOT/target/release/urd" "$@")
              fi
            }
            
            # URD with Zenoh integration
            urd-z() {
              (cd "$REPO_ROOT" && cargo run --bin urd --features zenoh-integration -- --enable-rpc "$@")
            }
            
            # Zenoh subscriber with topic filtering
            urd-zsub() {
              if [ $# -eq 0 ]; then
                # No arguments - subscribe to both topics
                (cd "$REPO_ROOT" && cargo run --bin zenoh_subscriber --features zenoh-integration)
              else
                # Topic specified - use it
                (cd "$REPO_ROOT" && cargo run --bin zenoh_subscriber --features zenoh-integration -- --topics="$1")
              fi
            }
            
            # General command client
            urd-command() {
              if [ -f "$REPO_ROOT/target/release/urd_command" ]; then
                "$REPO_ROOT/target/release/urd_command" "$@"
              elif [ -f "$REPO_ROOT/target/debug/urd_command" ]; then
                "$REPO_ROOT/target/debug/urd_command" "$@"
              else
                (cd "$REPO_ROOT" && cargo build --release --bin urd_command --features zenoh-integration && "$REPO_ROOT/target/release/urd_command" "$@")
              fi
            }
          '';
        };
      }
    );
}