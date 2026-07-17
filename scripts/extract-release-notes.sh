#!/usr/bin/env bash

set -euo pipefail

if (( $# < 1 || $# > 2 )); then
    echo "usage: $0 <version-tag> [release-notes-file]" >&2
    exit 2
fi

version="$1"
notes_file="${2:-RELEASE_NOTES.md}"

if [[ ! -r "$notes_file" ]]; then
    echo "release notes file is not readable: $notes_file" >&2
    exit 2
fi

awk -v version="$version" '
    function is_target_heading(line, target, boundary) {
        target = "## " version
        if (index(line, target) != 1) {
            return 0
        }

        boundary = substr(line, length(target) + 1, 1)
        return boundary == "" || boundary ~ /[[:space:](:]/
    }

    is_target_heading($0) {
        found = 1
        next
    }

    found && /^## v[0-9]/ {
        exit
    }

    found {
        lines[++line_count] = $0
    }

    END {
        if (!found) {
            print "release notes heading not found: ## " version > "/dev/stderr"
            exit 1
        }

        first_line = 1
        while (first_line <= line_count &&
               (lines[first_line] == "" || lines[first_line] == "---")) {
            first_line++
        }

        while (line_count >= first_line &&
               (lines[line_count] == "" || lines[line_count] == "---")) {
            line_count--
        }

        if (first_line > line_count) {
            print "release notes section is empty: ## " version > "/dev/stderr"
            exit 1
        }

        for (i = first_line; i <= line_count; i++) {
            print lines[i]
        }
    }
' "$notes_file"
