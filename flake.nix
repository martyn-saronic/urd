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

            # Python environment
            (python3.withPackages (ps: with ps; [
              pip
              setuptools
              wheel
            ]))

            # MQTT tooling
            mosquitto

            # Docker tools
            docker
            docker-compose
          ];
          
          shellHook = ''
            echo "URD Development Environment"
            echo "Rust: $(rustc --version)"
            echo "Python: $(python3 --version)"

            REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
            export PYTHONPATH="$REPO_ROOT:$PYTHONPATH"
            export DEFAULT_CONFIG_PATH=''${DEFAULT_CONFIG_PATH:-"$REPO_ROOT/config/default_config.yaml"}

            echo ""
            echo "Commands:"
            echo "  start-sim   - Start UR10e simulator (Docker)"
            echo "  stop-sim    - Stop simulator"
            echo "  ur-init     - Power on and initialize robot"
            echo "  urd         - Run the URD daemon"
            echo "  cargo build - Build"
            echo ""

            alias start-sim="$REPO_ROOT/scripts/start-sim.sh"
            alias stop-sim="$REPO_ROOT/scripts/stop-sim.sh"
            alias ur-init="$REPO_ROOT/scripts/ur-init.sh"

            urd() {
              if [ -f "$REPO_ROOT/target/release/urd" ]; then
                "$REPO_ROOT/target/release/urd" "$@"
              elif [ -f "$REPO_ROOT/target/debug/urd" ]; then
                "$REPO_ROOT/target/debug/urd" "$@"
              else
                (cd "$REPO_ROOT" && cargo build --bin urd && "$REPO_ROOT/target/debug/urd" "$@")
              fi
            }
          '';
        };
      }
    );
}