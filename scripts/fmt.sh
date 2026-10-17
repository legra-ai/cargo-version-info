#!/bin/sh
# Local hooks and CI share one explicit formatter.
formatter_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) || exit 1
cd "$formatter_root" || exit 1
IFS= read -r formatter_toolchain < rustfmt-toolchain || exit 1
exec cargo "+$formatter_toolchain" fmt --all -- "$@"
