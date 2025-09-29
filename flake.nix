{
  description = "URD Core - IPC-agnostic Universal Robots control library";

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
            
            # Development utilities
            just
            
            # Python environment with URD SDK
            (python3.withPackages (ps: with ps; [
              pip
              setuptools
              wheel
            ]))
            
            # Docker tools for robot simulation
            docker
            docker-compose
          ];
          
          shellHook = ''
            echo "🤖 URD Core - IPC-agnostic Universal Robots Library"
            echo "Rust: $(rustc --version)"
            echo "Python: $(python3 --version)"
            echo "Docker: $(docker --version)"
            
            # Get the repository root directory
            REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
            
            # Set up Python environment with URD SDK
            export PYTHONPATH="$REPO_ROOT:$PYTHONPATH"
            
            # Set default config path environment variable (if not already set)
            export DEFAULT_CONFIG_PATH=''${DEFAULT_CONFIG_PATH:-"$REPO_ROOT/config/default_config.yaml"}
            
            echo ""
            echo "📦 URD Core Library:"
            echo "  Pure robot control functionality, no transport dependencies"
            echo "  Can be embedded in any application or transport layer"
            echo ""
            echo "🚀 Quick Start:"
            echo "  cargo build         - Build URD Core library"
            echo "  cargo test          - Run tests"
            echo "  cargo doc --open    - View documentation"
            echo ""
            echo "🔧 Utilities:"
            echo "  start-sim           - Start UR10e simulator" 
            echo "  stop-sim            - Stop UR10e simulator"
            echo "  ur-init             - Power on and initialize UR robot"
            echo "  test-urd-py         - Test URD Python SDK"
            echo ""
            
            # Create shell aliases for convenience
            alias start-sim="$REPO_ROOT/scripts/start-sim.sh"
            alias stop-sim="$REPO_ROOT/scripts/stop-sim.sh"
            alias ur-init="$REPO_ROOT/scripts/ur-init.sh"
            
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
                echo "Make sure you're in a nix development environment with Python dependencies"
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
print('URD Core library is available for embedding in transport layers')
"
              fi
            }
          '';
        };
      }
    );
}