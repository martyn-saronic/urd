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
            
            # Python environment with custom packages
            (python3.withPackages (ps: with ps; [
              pip
              setuptools
              wheel
              # We'll install eclipse-zenoh via pip in shellHook since it's not in nixpkgs
            ]))
            
            # Docker tools
            docker
            docker-compose
          ];
          
          shellHook = ''
            echo "🤖 UR10e Development Environment"
            echo "Rust: $(rustc --version)"
            echo "Python: $(python3 --version)"
            echo "Docker: $(docker --version)"
            
            # Get the repository root directory
            REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
            
            # Set up Python environment with URD SDK
            export PYTHONPATH="$REPO_ROOT:$PYTHONPATH"
            
            # Set up a local Python environment for URD SDK
            if [ ! -d "$REPO_ROOT/.urd_env" ]; then
              echo "Setting up URD Python environment..."
              python3 -m venv "$REPO_ROOT/.urd_env" >/dev/null 2>&1
              "$REPO_ROOT/.urd_env/bin/pip" install eclipse-zenoh >/dev/null 2>&1
              if [ $? -eq 0 ]; then
                echo "✓ URD Python environment created with zenoh"
              else
                echo "⚠ Failed to create URD Python environment"
              fi
            fi
            
            # Set up Python aliases to use URD environment if available
            if [ -f "$REPO_ROOT/.urd_env/bin/python" ]; then
              alias python-urd="PYTHONPATH='$REPO_ROOT:\$PYTHONPATH' $REPO_ROOT/.urd_env/bin/python"
              alias python3-urd="PYTHONPATH='$REPO_ROOT:\$PYTHONPATH' $REPO_ROOT/.urd_env/bin/python"
              echo "✓ URD Python environment available (use python-urd or python3-urd)"
            else
              alias python-urd="PYTHONPATH='$REPO_ROOT:\$PYTHONPATH' python3"
              alias python3-urd="PYTHONPATH='$REPO_ROOT:\$PYTHONPATH' python3"
            fi
            
            echo "Available: Rust toolchain, Python 3 + zenoh, Docker, URD Python SDK"
            echo ""
            echo "Commands:"
            echo "  start-sim      - Start UR10e simulator"
            echo "  stop-sim       - Stop UR10e simulator"
            echo "  ur-init        - Power on and initialize UR robot"
            echo "  urd            - URD RPC service (Zenoh-based) - PRIMARY SERVICE"
            echo "  urd-cli        - Dynamic command client (usage: urd-cli <SERVICE> [SERVICE_ARGS...])"
            echo "  python3-urd    - Python with URD SDK and zenoh available"
            echo "  test-urd-py    - Test URD Python SDK with running RPC service"
            echo "  cargo build    - Build Rust workspace"
            echo ""
            
            # Create shell aliases for convenience
            
            # Set default config path environment variable (if not already set)
            export DEFAULT_CONFIG_PATH=''${DEFAULT_CONFIG_PATH:-"$REPO_ROOT/config/default_config.yaml"}
            
            alias start-sim="$REPO_ROOT/scripts/start-sim.sh"
            alias stop-sim="$REPO_ROOT/scripts/stop-sim.sh"
            alias ur-init="$REPO_ROOT/scripts/ur-init.sh"
            
            
            # URD RPC service (Zenoh-based) 
            urd() {
              if [ -f "$REPO_ROOT/target/release/urd" ]; then
                "$REPO_ROOT/target/release/urd" "$@"
              elif [ -f "$REPO_ROOT/target/debug/urd" ]; then
                "$REPO_ROOT/target/debug/urd" "$@"
              else
                (cd "$REPO_ROOT" && cargo build --release --bin urd && "$REPO_ROOT/target/release/urd" "$@")
              fi
            }
            
            # Dynamic command client
            urd-cli() {
              if [ -f "$REPO_ROOT/target/release/urd_cli" ]; then
                "$REPO_ROOT/target/release/urd_cli" "$@"
              elif [ -f "$REPO_ROOT/target/debug/urd_cli" ]; then
                "$REPO_ROOT/target/debug/urd_cli" "$@"
              else
                (cd "$REPO_ROOT" && cargo build --release --bin urd_cli && "$REPO_ROOT/target/release/urd_cli" "$@")
              fi
            }
            
            
            # Test URD Python SDK
            test-urd-py() {
              echo "Testing URD Python SDK..."
              
              # Choose the right Python interpreter
              local python_cmd="python3"
              if [ -f "$REPO_ROOT/.urd_env/bin/python" ]; then
                python_cmd="$REPO_ROOT/.urd_env/bin/python"
              fi
              
              # Test basic import
              if ! PYTHONPATH="$REPO_ROOT:$PYTHONPATH" $python_cmd -c "import urd_py" 2>/dev/null; then
                echo "✗ URD Python SDK not importable"
                echo "Make sure you're in the nix development environment: nix develop"
                return 1
              fi
              
              echo "✓ URD Python SDK imported successfully"
              
              # Run demo if available
              if [ -f "$REPO_ROOT/examples/python_sdk_demo.py" ]; then
                echo "Running Python SDK demo..."
                PYTHONPATH="$REPO_ROOT:$PYTHONPATH" $python_cmd "$REPO_ROOT/examples/python_sdk_demo.py"
              else
                echo "Demo file not found, running basic test..."
                PYTHONPATH="$REPO_ROOT:$PYTHONPATH" $python_cmd -c "
import urd_py
print('✓ URD Python SDK version:', urd_py.__version__)
print('Available classes:', [name for name in dir(urd_py) if not name.startswith('_')])
print('To test with actual RPC service, make sure urd-rpc is running')
"
              fi
            }
          '';
        };
      }
    );
}