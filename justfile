test:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo test
    cargo build --release
    failed=0
    echo "Testing data/ folder..."
    for dir in data/*/; do
        name=$(basename "$dir")
        actual=$(./target/release/payx "$dir/in.csv" 2>/dev/null | sort)
        expected=$(sort "$dir/out.csv")
        if [ "$actual" = "$expected" ]; then
            echo "PASS: $name"
        else
            echo "FAIL: $name"
            diff <(echo "$actual") <(echo "$expected") || true
            failed=1
        fi
    done
    exit $failed
