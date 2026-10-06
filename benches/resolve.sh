#!/bin/sh
#
# In the baseline checkout:
#   cargo test --release --test resolve --no-run
#
# In the current checkout (requires hyperfine in PATH):
#   cargo bench -- /path/to/baseline/target/release/deps/

set -eu

if [ "$#" -ne 1 ]; then
	printf 'usage: cargo bench -- baseline-directory\n' >&2
	exit 1
fi

# Use the newest executable resolve binary.
baseline=$(
	ls -t "${1%/}"/resolve-* 2>/dev/null |
	while IFS= read -r candidate; do
		[ -f "$candidate" ] && [ -x "$candidate" ] || continue
		printf '%s\n' "$candidate"
		break
	done
)
if [ -z "$baseline" ]; then
	printf 'No resolver test binary in %s\n' "$1" >&2
	printf 'Run cargo test --release --test resolve --no-run in the baseline checkout.\n' >&2
	exit 1
fi

if ! build=$(cargo test --release --test resolve --no-run --color never 2>&1); then
	printf '%s\n' "$build" >&2
	exit 1
fi
binary=$(printf '%s\n' "$build" |
    sed -n 's/^ *Executable .* (\(.*\))$/\1/p')

hyperfine --warmup 2 --runs 10 --shell=none \
    "\"$baseline\" resolve_full_scan --exact" \
    "\"$binary\" resolve_full_scan --exact"

if [ -x /usr/bin/time ]; then
	case $(uname -s) in
	Darwin) time_flag=-l ;;
	Linux) time_flag=-v ;;
	*) exit 0 ;;
	esac
	for executable in "$baseline" "$binary"; do
		printf '\nMemory usage: %s\n' "$executable"
		/usr/bin/time "$time_flag" "$executable" \
		    resolve_full_scan --exact >/dev/null
	done
fi
