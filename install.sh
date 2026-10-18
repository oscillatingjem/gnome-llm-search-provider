#!/usr/bin/env bash
# Install the GNOME LLM search provider for the current user.
# Options are passed to `llm-search install` (see `bin/llm-search help`).
# Set LLM_SEARCH_HOME to choose where config lives (default ~/.llm-search).
set -euo pipefail
exec "$(dirname "$(readlink -f "$0")")/bin/llm-search" install "$@"
